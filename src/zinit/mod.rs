pub mod config;
pub mod ord;
use crate::manager::{Log, Logs, Process, ProcessManager};
use crate::zinit::ord::ProcessDAG;
use crate::zinit::ord::{service_dependency_order, DUMMY_ROOT};
use anyhow::Result;
use futures::future::join_all;
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

const DEFAULT_SHUTDOWN_TIMEOUT: u64 = 10; // in seconds

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

struct Watched<T> {
    v: Arc<T>,
    tx: watch::Sender<Arc<T>>,
}

type Changed<T> = mpsc::Receiver<Arc<T>>;

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
        if self.tx.send(v).is_err() {
            //nothing to do, we don't care if
            //no watchers are watching. this should not block
        }
    }

    pub fn get(&self) -> &T {
        &self.v
    }

    pub async fn watch(&self) -> Changed<T> {
        // we have channel of 2 slots so it can hold at least the current value, and last change happens since the subscription.
        let (tx, rx) = mpsc::channel(2);
        let mut w = self.tx.subscribe();
        let v = Arc::clone(&self.v);
        // if channel is closed, the send() will always fail so the one watching
        // is not interested anymore, hence we can return safely
        tokio::spawn(async move {
            if tx.send(v).await.is_err() {
                return;
            };
            while w.changed().await.is_ok() {
                let x = (*w.borrow()).clone();
                if tx.send(x).await.is_err() {
                    return;
                };
            }
        });
        rx
    }
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

pub struct ZInitStatus {
    pub pid: Pid,
    // config is the service configuration
    pub service: config::Service,
    // target is the target state of the service (up, down)
    pub target: Target,
    pub scheduled: bool,
    pub state: State,
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

    pub fn status(&self) -> ZInitStatus {
        ZInitStatus {
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

    pub async fn logs(&self) -> Result<Logs> {
        self.pm.stream().await
    }

    pub async fn monitor<S: Into<String>>(&self, name: S, service: config::Service) -> Result<()> {
        if *self.shutdown.read().await {
            bail!(ZInitError::ShuttingDown);
        }
        let name = name.into();
        let mut services = self.services.write().await;

        if services.contains_key(&name) {
            bail!(ZInitError::ServiceAlreadyMonitored { name })
        }

        let service = Arc::new(RwLock::new(ZInitService::new(service, State::Unknown)));
        services.insert(name.clone(), Arc::clone(&service));
        let m = self.clone();
        debug!("service '{}' monitored", name);
        tokio::spawn(m.watch(name, service));
        Ok(())
    }

    pub async fn status<S: AsRef<str>>(&self, name: S) -> Result<ZInitStatus> {
        let table = self.services.read().await;
        let service = table.get(name.as_ref());

        let service = match service {
            Some(service) => service,
            None => bail!(ZInitError::UnknownService {
                name: name.as_ref().into()
            }),
        };

        let service = service.read().await.status();
        Ok(service)
    }
    async fn wait_kill(
        name: String,
        ch: mpsc::UnboundedSender<String>,
        mut rx: Changed<State>,
        shutdown_timeout: u64,
    ) -> Result<()> {
        debug!("kill_wait {}", name);
        let fut = timeout(
            std::time::Duration::from_secs(shutdown_timeout),
            async move {
                while let Some(new_state) = rx.recv().await {
                    if *new_state != State::Running && *new_state != State::Spawned {
                        return;
                    }
                }
            },
        );
        let _ = fut.await;
        debug!("sending to the death channel {}", name.clone());
        ch.send(name.clone())?;
        Ok(())
    }
    async fn kill_process_tree(
        &self,
        mut dag: ProcessDAG,
        mut state_channels: HashMap<String, Changed<State>>,
        mut shutdown_timeouts: HashMap<String, u64>,
    ) -> Result<()> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        tx.send(DUMMY_ROOT.into())?;
        let mut futs = vec![];
        let mut count = dag.adj.len();
        while let Some(name) = rx.recv().await {
            debug!(
                "{} has been killed (or was inactive) adding its children",
                name
            );
            for child in dag.adj.get(&name).unwrap_or(&Vec::new()) {
                let child_indegree: &mut u32 = dag.indegree.entry(child.clone()).or_insert(0);
                *child_indegree -= 1;
                debug!(
                    "decrementing child {} indegree to {}",
                    child, child_indegree
                );
                if *child_indegree == 0 {
                    let state_rx = state_channels.remove(child);
                    if state_rx.is_none() {
                        // not an active service
                        tx.send(child.to_string())?;
                        continue;
                    }
                    let shutdown_timeout = shutdown_timeouts.remove(child);
                    let fut = tokio::spawn(Self::wait_kill(
                        child.to_string(),
                        tx.clone(),
                        state_rx.unwrap(),
                        shutdown_timeout.unwrap_or(DEFAULT_SHUTDOWN_TIMEOUT),
                    ));
                    futs.push(fut);

                    // no guarantee that rx started listening?
                    // but this is how it's used in the docs
                    // not fatal though: the service will timeout
                    // because the state change won't be reported
                    // and the flow will continue. so the problem
                    // is only unnecessary waiting
                    if let Err(e) = self.stop(child.clone()).await {
                        error!("couldn't shutdown service {}: {}", name.clone(), e);
                    }
                }
            }
            count -= 1;
            if count == 0 {
                break;
            }
        }
        join_all(futs).await;
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<()> {
        info!("shutting down");
        *self.shutdown.write().await = true;
        let mut state_channels: HashMap<String, Changed<State>> = HashMap::new();
        let mut shutdown_timeouts: HashMap<String, u64> = HashMap::new();
        let table = self.services.read().await;
        for (name, service) in table.iter() {
            let service = service.read().await;
            if *service.state.get() == State::Running || *service.state.get() == State::Spawned {
                info!("service '{}' is scheduled for a shutdown", name);
                state_channels.insert(name.into(), service.state.watch().await);
                let mut timeout = service.service.shutdown_timeout;
                if timeout == 0 {
                    timeout = DEFAULT_SHUTDOWN_TIMEOUT;
                }
                shutdown_timeouts.insert(name.into(), timeout);
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
            nix::sys::reboot::reboot(nix::sys::reboot::RebootMode::RB_AUTOBOOT)?;
        }
        Ok(())
    }

    pub async fn stop<S: AsRef<str>>(&self, name: S) -> Result<()> {
        let table = self.services.read().await;
        let service = table.get(name.as_ref());

        let service = match service {
            Some(service) => service,
            None => bail!(ZInitError::UnknownService {
                name: name.as_ref().into()
            }),
        };
        let mut service = service.write().await;
        service.target = Target::Down;
        let signal = match signal::Signal::from_str(&service.service.signal.stop.to_uppercase()) {
            Ok(signal) => signal,
            Err(err) => bail!(
                "unknown stop signal configured '{}': {}",
                service.service.signal.stop,
                err
            ),
        };

        if service.pid.as_raw() == 0 {
            return Ok(());
        }

        self.pm.signal(service.pid, signal)
    }

    pub async fn start<S: AsRef<str>>(&self, name: S) -> Result<()> {
        if *self.shutdown.read().await {
            bail!(ZInitError::ShuttingDown);
        }
        self.set(name.as_ref(), None, Some(Target::Up), None).await;
        let table = self.services.read().await;

        let service = match table.get(name.as_ref()) {
            Some(service) => service,
            None => bail!(ZInitError::UnknownService {
                name: name.as_ref().into()
            }),
        };

        let m = self.clone();
        tokio::spawn(m.watch(name.as_ref().into(), Arc::clone(service)));
        Ok(())
    }

    pub async fn forget<S: AsRef<str>>(&self, name: S) -> Result<()> {
        let mut table = self.services.write().await;
        let service = match table.get(name.as_ref()) {
            Some(service) => service,
            None => bail!(ZInitError::UnknownService {
                name: name.as_ref().into()
            }),
        };

        let service = service.read().await;
        if service.target == Target::Up || service.pid != Pid::from_raw(0) {
            bail!(ZInitError::ServiceISUp {
                name: name.as_ref().into()
            })
        }

        drop(service);
        table.remove(name.as_ref());
        Ok(())
    }

    pub async fn kill<S: AsRef<str>>(&self, name: S, signal: signal::Signal) -> Result<()> {
        let table = self.services.read().await;
        let service = match table.get(name.as_ref()) {
            Some(service) => service,
            None => bail!(ZInitError::UnknownService {
                name: name.as_ref().into()
            }),
        };

        let service = service.read().await;
        if service.pid == Pid::from_raw(0) {
            bail!(ZInitError::ServiceISDown {
                name: name.as_ref().into(),
            })
        }

        self.pm.signal(service.pid, signal)
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
                        dep, ps.state.get(), ps.service.one_shot
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
                        self.set(&name, Some(State::Running), None, None).await;
                        // release
                        self.notify.notify_waiters();
                        return;
                    }
                    // wait before we try again
                    time::sleep(std::time::Duration::from_secs(2)).await;
                }
                Err(_) => {
                    self.set(&name, Some(State::TestFailure), None, None).await;
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

    async fn set(
        &self,
        name: &str,
        state: Option<State>,
        target: Option<Target>,
        scheduled: Option<bool>,
    ) {
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

        if let Some(scheduled) = scheduled {
            service.scheduled = scheduled
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

                self.set(&name, Some(State::Blocked), None, None).await;
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

            let child = self
                .pm
                .run(
                    Process::new(&config.exec, &config.dir, Some(config.env.clone())),
                    log.clone(),
                )
                .await;

            let mut service = input.write().await;

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
            // to finish. so we allow other services to schedule
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
                let _ = self.notify.notify_waiters();
                break;
            }
            // we trying again in 2 seconds
            time::sleep(std::time::Duration::from_secs(2)).await;
        }

        self.set(&name, None, None, Some(false)).await;
    }
}
