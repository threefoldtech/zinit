pub mod config;
pub mod ord;
use crate::manager::{Log, Logs, Process, ProcessManager};
use crate::zinit::ord::ProcessDAG;
use crate::zinit::ord::{service_dependency_order, DUMMY_ROOT};
use anyhow::Result;
use async_recursion::async_recursion;
use config::DEFAULT_SHUTDOWN_TIMEOUT;
use nix::sys::reboot::RebootMode;
use nix::sys::signal;
use nix::sys::wait::WaitStatus;
use nix::unistd::Pid;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::sync::watch;
use tokio::sync::{Notify, RwLock};
use tokio::time;
use tokio::time::timeout;
use tokio_stream::{wrappers::WatchStream, StreamExt};

pub trait WaitStatusExt {
    fn success(&self) -> bool;
}

impl WaitStatusExt for WaitStatus {
    fn success(&self) -> bool {
        matches!(self, WaitStatus::Exited(_, code) if *code == 0)
    }
}
#[derive(Error, Debug)]
pub enum ZInitError {
    #[error("service name {name:?} unknown")]
    UnknownService { name: String },
    #[error("service {name:?} already monitored")]
    ServiceAlreadyMonitored { name: String },
    #[error("service {name:?} is up")]
    ServiceISUp { name: String },
    #[error("service {name:?} is down")]
    ServiceISDown { name: String },
    #[error("zinit is shutting down")]
    ShuttingDown,
}
/// Process is a representation of a scheduled/running
/// service
pub struct ZInitService {
    pub pid: Pid,
    // config is the service configuration
    pub service: config::Service,
    // target is the target state of the service (up, down)
    pub target: Target,
    pub scheduled: bool,
    state: Watched<State>,
}

type Watcher<T> = WatchStream<Arc<T>>;

struct Watched<T> {
    v: Arc<T>,
    tx: watch::Sender<Arc<T>>,
}

impl<T> Watched<T>
where
    T: Send + Sync + 'static,
{
    pub fn new(v: T) -> Self {
        let v = Arc::new(v);
        let (tx, _) = watch::channel(Arc::clone(&v));
        Self { v, tx }
    }

    pub fn set(&mut self, v: T) {
        let v = Arc::new(v);
        self.v = Arc::clone(&v);
        // update the value even when there are no receivers
        self.tx.send_replace(v);
    }

    pub fn get(&self) -> &T {
        &self.v
    }

    pub fn watcher(&self) -> Watcher<T> {
        WatchStream::new(self.tx.subscribe())
    }
}

pub enum ZInitStatus {
    Service(ServiceStatus),
    Group(GroupStatus),
}

pub struct ServiceStatus {
    pub pid: Pid,
    // config is the service configuration
    pub service: config::Service,
    // target is the target state of the service (up, down)
    pub target: Target,
    pub scheduled: bool,
    pub state: State,
}

pub struct GroupStatus {
    pub name: String,
    pub services: Vec<(String, ServiceStatus)>,
    // maybe add target and state based on services of the group?
}

impl ZInitService {
    fn new(service: config::Service, state: State) -> ZInitService {
        ZInitService {
            pid: Pid::from_raw(0),
            state: Watched::new(state),
            service,
            target: Target::Up,
            scheduled: false,
        }
    }

    pub fn status(&self) -> ServiceStatus {
        ServiceStatus {
            pid: self.pid,
            state: self.state.get().clone(),
            service: self.service.clone(),
            target: self.target.clone(),
            scheduled: self.scheduled,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Up,
    Down,
}
/// Service state
#[derive(Debug, PartialEq, Clone)]
pub enum State {
    // service is in an unknown state
    Unknown,
    /// Blocked means one or more dependencies hasn't been met yet. Service can stay in
    /// this state as long as at least one dependency is not in either Running, or Success
    Blocked,
    /// service has been started, but it didn't exit yet, or we didn't run the test command.
    Spawned,
    /// service has been started, and test command passed.
    Running,
    /// service has exited with success state, only one-shot can stay in this state
    Success,
    /// service exited with this error, only one-shot can stay in this state
    Error(WaitStatus),
    /// the service test command failed, this might (or might not) be replaced
    /// with an Error state later on once the service process itself exits
    TestFailure,
    /// Failure means the service has failed to spawn in a way that retyring
    /// won't help, like command line parsing error or failed to fork
    Failure,
}

type Table = HashMap<String, Arc<RwLock<ZInitService>>>;

#[derive(Clone)]
pub struct ZInit {
    pm: ProcessManager,
    services: Arc<RwLock<Table>>,
    notify: Arc<Notify>,
    shutdown: Arc<RwLock<bool>>,
    container: bool,
}

impl ZInit {
    pub fn new(cap: usize, container: bool) -> ZInit {
        ZInit {
            pm: ProcessManager::new(cap),
            services: Arc::new(RwLock::new(Table::new())),
            notify: Arc::new(Notify::new()),
            shutdown: Arc::new(RwLock::new(false)),
            container,
        }
    }

    pub fn serve(&self) {
        self.pm.start();
        if self.container {
            let m = self.clone();
            tokio::spawn(m.on_signal());
        }
    }

    async fn on_signal(self) {
        use tokio::signal::unix;

        let mut term = unix::signal(unix::SignalKind::terminate()).unwrap();
        let mut int = unix::signal(unix::SignalKind::interrupt()).unwrap();
        let mut hup = unix::signal(unix::SignalKind::hangup()).unwrap();
        tokio::select! {
            _ = term.recv() => {},
            _ = int.recv() => {},
            _ = hup.recv() => {},
        };

        debug!("shutdown signal received");
        let _ = self.shutdown().await;
    }

    pub async fn logs(&self, follow: bool) -> Logs {
        self.pm.stream(follow).await
    }

    pub async fn monitor<S: Into<String>>(&self, name: S, mut entry: config::Entry) -> Result<()> {
        if *self.shutdown.read().await {
            bail!(ZInitError::ShuttingDown);
        }
        let name = name.into();
        let services = self.services.read().await;

        if services.contains_key(&name) {
            bail!(ZInitError::ServiceAlreadyMonitored { name })
        }
        if let config::Entry::Directory(ref mut svcs) = entry {
            svcs.retain(|k, _| !services.contains_key(k));
            // if nothing new to monitor in a group return an error
            if svcs.is_empty() {
                bail!(ZInitError::ServiceAlreadyMonitored { name })
            }
        }
        drop(services);
        match entry {
            config::Entry::Service(service) => self.monitor_service(name, service).await,
            config::Entry::Directory(services) => self.monitor_dir(services).await,
        }
    }

    async fn monitor_service<S: Into<String>>(
        &self,
        name: S,
        service: config::Service,
    ) -> Result<()> {
        let name = name.into();
        let mut services = self.services.write().await;
        let service = Arc::new(RwLock::new(ZInitService::new(service, State::Unknown)));
        services.insert(name.clone(), Arc::clone(&service));
        let m = self.clone();
        debug!("service '{}' monitored", name);
        tokio::spawn(m.watch(name, service));
        Ok(())
    }

    #[async_recursion]
    async fn monitor_dir(&self, services: config::Services) -> Result<()> {
        for (name, service) in services {
            match service {
                config::Entry::Service(service) => {
                    self.monitor_service(name, service).await?;
                }
                config::Entry::Directory(services) => {
                    self.monitor_dir(services).await?;
                }
            }
        }
        Ok(())
    }

    pub async fn status<S: AsRef<str>>(&self, name: S) -> Result<ZInitStatus> {
        let table = self.services.read().await;
        let service = table.get(name.as_ref());
        if let Some(service) = service {
            return Ok(ZInitStatus::Service(service.read().await.status()));
        }
        drop(table);

        let services = services_with_prefix(name.as_ref(), self.services.clone()).await;
        if services.is_empty() {
            bail!(ZInitError::UnknownService {
                name: name.as_ref().into()
            })
        }
        let mut status = GroupStatus {
            name: name.as_ref().to_string(),
            services: vec![],
        };

        for (name, service) in services {
            status.services.push((name, service.read().await.status()));
        }

        Ok(ZInitStatus::Group(status))
    }

    async fn kill_wait(
        self,
        name: String,
        ch: mpsc::UnboundedSender<String>,
        mut rx: Watcher<State>,
        shutdown_timeout: u64,
    ) {
        debug!("kill_wait {}", name);
        let fut = timeout(
            std::time::Duration::from_secs(shutdown_timeout),
            async move {
                while let Some(state) = rx.next().await {
                    if *state != State::Running && *state != State::Spawned {
                        return;
                    }
                }
            },
        );
        let stop_result = self.stop(name.clone()).await;
        match stop_result {
            Ok(_) => {
                let _ = fut.await;
            }
            Err(e) => error!("couldn't stop service {}: {}", name.clone(), e),
        }
        debug!("sending to the death channel {}", name.clone());
        if let Err(e) = ch.send(name.clone()) {
            error!(
                "error: couldn't send the service {} to the shutdown loop: {}",
                name, e
            );
        }
    }

    async fn kill_process_tree(
        &self,
        mut dag: ProcessDAG,
        mut state_channels: HashMap<String, Watcher<State>>,
        mut shutdown_timeouts: HashMap<String, u64>,
    ) -> Result<()> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        tx.send(DUMMY_ROOT.into())?;
        let mut count = dag.count;
        while let Some(name) = rx.recv().await {
            debug!(
                "{} has been killed (or was inactive) adding its children",
                name
            );
            for child in dag.adj.get(&name).unwrap_or(&Vec::new()) {
                let child_indegree: &mut u32 = dag.indegree.entry(child.clone()).or_default();
                *child_indegree -= 1;
                debug!(
                    "decrementing child {} indegree to {}",
                    child, child_indegree
                );
                if *child_indegree == 0 {
                    let watcher = state_channels.remove(child);
                    if watcher.is_none() {
                        // not an active service
                        tx.send(child.to_string())?;
                        continue;
                    }
                    let shutdown_timeout = shutdown_timeouts.remove(child);
                    tokio::spawn(Self::kill_wait(
                        self.clone(),
                        child.to_string(),
                        tx.clone(),
                        watcher.unwrap(),
                        shutdown_timeout.unwrap_or(DEFAULT_SHUTDOWN_TIMEOUT),
                    ));
                }
            }
            count -= 1;
            if count == 0 {
                break;
            }
        }
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<()> {
        info!("shutting down");
        self.power(RebootMode::RB_POWER_OFF).await
    }

    pub async fn reboot(&self) -> Result<()> {
        info!("rebooting");
        self.power(RebootMode::RB_AUTOBOOT).await
    }

    async fn power(&self, mode: RebootMode) -> Result<()> {
        *self.shutdown.write().await = true;
        let mut state_channels: HashMap<String, Watcher<State>> = HashMap::new();
        let mut shutdown_timeouts: HashMap<String, u64> = HashMap::new();
        let table = self.services.read().await;
        for (name, service) in table.iter() {
            let service = service.read().await;
            if *service.state.get() == State::Running || *service.state.get() == State::Spawned {
                info!("service '{}' is scheduled for a shutdown", name);
                state_channels.insert(name.into(), service.state.watcher());
                shutdown_timeouts.insert(name.into(), service.service.shutdown_timeout);
            }
        }
        drop(table);
        let dag = service_dependency_order(self.services.clone()).await;
        self.kill_process_tree(dag, state_channels, shutdown_timeouts)
            .await?;
        nix::unistd::sync();
        if self.container {
            std::process::exit(0);
        } else {
            nix::sys::reboot::reboot(mode)?;
        }
        Ok(())
    }

    pub async fn stop<S: AsRef<str>>(&self, name: S) -> Result<()> {
        let services = services_with_prefix(name.as_ref(), self.services.clone()).await;
        if services.is_empty() {
            bail!(ZInitError::UnknownService {
                name: name.as_ref().into()
            })
        }
        for (_, service) in services {
            let mut service = service.write().await;
            service.target = Target::Down;
            let signal = match signal::Signal::from_str(&service.service.signal.stop.to_uppercase())
            {
                Ok(signal) => signal,
                Err(err) => bail!(
                    "unknown stop signal configured '{}': {}",
                    service.service.signal.stop,
                    err
                ),
            };
            if service.pid.as_raw() == 0 {
                continue;
            }
            self.pm.signal(service.pid, signal)?;
        }

        Ok(())
    }

    pub async fn start<S: AsRef<str>>(&self, name: S) -> Result<()> {
        if *self.shutdown.read().await {
            bail!(ZInitError::ShuttingDown);
        }
        let services = services_with_prefix(name.as_ref(), self.services.clone()).await;
        if services.is_empty() {
            bail!(ZInitError::UnknownService {
                name: name.as_ref().into()
            })
        }
        for (name, service) in services {
            self.set(name.as_ref(), None, Some(Target::Up)).await;

            let m = self.clone();
            tokio::spawn(m.watch(name, service));
        }
        Ok(())
    }

    pub async fn forget<S: AsRef<str>>(&self, name: S) -> Result<()> {
        let services = services_with_prefix(name.as_ref(), self.services.clone()).await;
        if services.is_empty() {
            bail!(ZInitError::UnknownService {
                name: name.as_ref().into()
            })
        }
        let mut table = self.services.write().await;
        for (name, service) in services {
            let service = service.read().await;
            if service.target == Target::Up || service.pid != Pid::from_raw(0) {
                bail!(ZInitError::ServiceISUp { name })
            }

            drop(service);
            table.remove(&name);
        }

        Ok(())
    }

    pub async fn kill<S: AsRef<str>>(&self, name: S, signal: signal::Signal) -> Result<()> {
        let services = services_with_prefix(name.as_ref(), self.services.clone()).await;
        if services.is_empty() {
            bail!(ZInitError::UnknownService {
                name: name.as_ref().into()
            })
        }
        let mut all_down = true;
        for (_, service) in services {
            let service = service.read().await;
            if service.pid == Pid::from_raw(0) {
                continue;
            }
            all_down = false;
            self.pm.signal(service.pid, signal)?;
        }
        if all_down {
            bail!(ZInitError::ServiceISDown {
                name: name.as_ref().into(),
            })
        }
        Ok(())
    }

    pub async fn list(&self) -> Result<Vec<String>> {
        let table = self.services.read().await;

        Ok(table.keys().map(|k| k.into()).collect())
    }

    async fn can_schedule(&self, service: &config::Service) -> bool {
        let mut can = true;
        let table = self.services.read().await;
        for dep in service.after.iter() {
            can = match table.get(dep) {
                Some(ps) => {
                    let ps = ps.read().await;
                    debug!(
                        "- service {} is {:?} oneshot: {}",
                        dep,
                        ps.state.get(),
                        ps.service.one_shot
                    );
                    match ps.state.get() {
                        State::Running if !ps.service.one_shot => true,
                        State::Success => true,
                        _ => false,
                    }
                }
                //depending on an undefined service. This still can be resolved later
                //by monitoring the dependency in the future.
                None => false,
            };

            // if state is blocked, we can break the loop
            if !can {
                break;
            }
        }

        can
    }

    async fn test<S: Into<String>>(self, name: S, cfg: config::Service) {
        let name = name.into();
        loop {
            let result = self.test_once(&name, &cfg).await;

            match result {
                Ok(result) => {
                    if result {
                        self.set(&name, Some(State::Running), None).await;
                        // release
                        self.notify.notify_waiters();
                        return;
                    }
                    // wait before we try again
                    time::sleep(std::time::Duration::from_secs(2)).await;
                }
                Err(_) => {
                    self.set(&name, Some(State::TestFailure), None).await;
                }
            }
        }
    }

    async fn test_once<S: AsRef<str>>(&self, name: S, cfg: &config::Service) -> Result<bool> {
        if cfg.test.is_empty() {
            return Ok(true);
        }

        let log = match cfg.log {
            config::Log::None => Log::None,
            config::Log::Stdout => Log::Stdout,
            config::Log::Ring => Log::Ring(format!("{}/test", name.as_ref())),
        };

        let test = self
            .pm
            .run(
                Process::new(&cfg.test, &cfg.dir, Some(cfg.env.clone())),
                log.clone(),
            )
            .await?;

        let status = test.wait().await?;
        if status.success() {
            return Ok(true);
        }

        Ok(false)
    }

    async fn set(&self, name: &str, state: Option<State>, target: Option<Target>) {
        let table = self.services.read().await;
        let service = match table.get(name) {
            Some(service) => service,
            None => return,
        };

        let mut service = service.write().await;
        if let Some(state) = state {
            service.state.set(state);
        }

        if let Some(target) = target {
            service.target = target;
        }
    }

    async fn watch(self, name: String, input: Arc<RwLock<ZInitService>>) {
        let name = name.clone();

        let mut service = input.write().await;
        if service.target == Target::Down {
            debug!("service '{}' target is down", name);
            return;
        }

        if service.scheduled {
            debug!("service '{}' already scheduled", name);
            return;
        }

        service.scheduled = true;
        drop(service);

        loop {
            let name = name.clone();

            let service = input.read().await;
            // early check if service is down, so we don't have to do extra checks
            if service.target == Target::Down {
                // we check target in loop in case service have
                // been set down.
                break;
            }
            let config = service.service.clone();
            // we need to spawn this service now, but is it ready?
            // are all dependent services are running ?

            // so we drop the table to give other services
            // chance to acquire the lock and schedule themselves
            drop(service);

            'checks: loop {
                let sig = self.notify.notified();
                debug!("checking {} if it can schedule", name);
                if self.can_schedule(&config).await {
                    debug!("service {} can schedule", name);
                    break 'checks;
                }

                self.set(&name, Some(State::Blocked), None).await;
                // don't even care if i am lagging
                // as long i am notified that some services status
                // has changed
                debug!("service {} is blocked, waiting release", name);
                sig.await;
            }

            let log = match config.log {
                config::Log::None => Log::None,
                config::Log::Stdout => Log::Stdout,
                config::Log::Ring => Log::Ring(name.clone()),
            };

            let mut service = input.write().await;
            // we check again in case target has changed. Since we had to release the lock
            // earlier to not block locking on this service (for example if a stop was called)
            // while the service was waiting for dependencies.
            // the lock is kept until the spawning and the update of the pid.
            if service.target == Target::Down {
                // we check target in loop in case service have
                // been set down.
                break;
            }

            let child = self
                .pm
                .run(
                    Process::new(&config.exec, &config.dir, Some(config.env.clone())),
                    log.clone(),
                )
                .await;

            let child = match child {
                Ok(child) => {
                    service.state.set(State::Spawned);
                    service.pid = child.pid;
                    child
                }
                Err(err) => {
                    // so, spawning failed. and nothing we can do about it
                    // this can be duo to a bad command or exe not found.
                    // set service to failure.
                    error!("service {} failed to start: {}", name, err);
                    service.state.set(State::Failure);
                    break;
                }
            };

            if config.one_shot {
                service.state.set(State::Running);
            }
            // we don't lock the here here because this can take forever
            // to finish. so we allow other operation on the service (for example)
            // status and stop operations.
            drop(service);

            let mut handler = None;
            if !config.one_shot {
                let m = self.clone();
                handler = Some(tokio::spawn(m.test(name.clone(), config.clone())));
            }

            let result = child.wait().await;
            if let Some(handler) = handler {
                handler.abort();
            }

            let mut service = input.write().await;
            service.pid = Pid::from_raw(0);
            match result {
                Err(err) => {
                    error!("failed to read service '{}' status: {}", name, err);
                    service.state.set(State::Unknown);
                }
                Ok(status) => service.state.set(match status.success() {
                    true => State::Success,
                    false => State::Error(status),
                }),
            };

            drop(service);
            if config.one_shot {
                // we don't need to restart the service anymore
                self.notify.notify_waiters();
                break;
            }
            // we trying again in 2 seconds
            time::sleep(std::time::Duration::from_secs(2)).await;
        }

        let mut service = input.write().await;
        service.scheduled = false;
    }
}

async fn services_with_prefix<S: AsRef<str>>(
    prefix: S,
    services: Arc<RwLock<Table>>,
) -> HashMap<String, Arc<RwLock<ZInitService>>> {
    let mut v = HashMap::new();
    let table = services.read().await;
    if let Some(service) = table.get(prefix.as_ref()) {
        v.insert(prefix.as_ref().to_string(), service.clone());
        return v;
    }
    let prefix = format!("{}/", prefix.as_ref());
    for (name, service) in table.iter() {
        if name.starts_with(&prefix) {
            v.insert(name.clone(), service.clone());
        }
    }
    v
}
