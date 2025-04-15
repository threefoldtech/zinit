use crate::manager::{Log, Logs, Process, ProcessManager};
use crate::zinit::config;
use crate::zinit::errors::ZInitError;
use crate::zinit::ord::{service_dependency_order, ProcessDAG, DUMMY_ROOT};
use crate::zinit::service::ZInitService;
use crate::zinit::state::{State, Target};
use crate::zinit::types::{ServiceTable, Watcher};

// Define a local extension trait for WaitStatus
trait WaitStatusExt {
    fn success(&self) -> bool;
}

impl WaitStatusExt for WaitStatus {
    fn success(&self) -> bool {
        matches!(self, WaitStatus::Exited(_, 0))
    }
}
use anyhow::Result;
use jsonrpsee::tracing::{debug, error, info};
use nix::sys::reboot::RebootMode;
use nix::sys::signal;
use nix::sys::wait::WaitStatus;
use nix::unistd::Pid;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{mpsc, Notify, RwLock};
use tokio::time;
use tokio::time::timeout;
use tokio_stream::StreamExt;

/// Manages the lifecycle of services
#[derive(Clone)]
pub struct LifecycleManager {
    /// Process manager for spawning and managing processes
    pm: ProcessManager,

    /// Table of services
    services: Arc<RwLock<ServiceTable>>,

    /// Notification for service state changes
    notify: Arc<Notify>,

    /// Whether the system is shutting down
    shutdown: Arc<RwLock<bool>>,

    /// Whether running in container mode
    container: bool,
}

impl LifecycleManager {
    /// Create a new lifecycle manager
    pub fn new(
        pm: ProcessManager,
        services: Arc<RwLock<ServiceTable>>,
        notify: Arc<Notify>,
        shutdown: Arc<RwLock<bool>>,
        container: bool,
    ) -> Self {
        Self {
            pm,
            services,
            notify,
            shutdown,
            container,
        }
    }

    /// Get a reference to the process manager
    pub fn process_manager(&self) -> &ProcessManager {
        &self.pm
    }

    /// Check if running in container mode
    pub fn is_container_mode(&self) -> bool {
        self.container
    }

    /// Get logs from the process manager
    pub async fn logs(&self, follow: bool) -> Logs {
        self.pm.stream(follow).await
    }

    /// Monitor a service
    pub async fn monitor<S: Into<String>>(&self, name: S, service: config::Service) -> Result<()> {
        if *self.shutdown.read().await {
            return Err(ZInitError::ShuttingDown.into());
        }

        let name = name.into();
        let mut services = self.services.write().await;

        if services.contains_key(&name) {
            return Err(ZInitError::service_already_monitored(name).into());
        }

        let service = Arc::new(RwLock::new(ZInitService::new(service, State::Unknown)));
        services.insert(name.clone(), Arc::clone(&service));

        let lifecycle = self.clone_lifecycle();
        debug!("service '{}' monitored", name);
        tokio::spawn(lifecycle.watch_service(name, service));

        Ok(())
    }

    /// Get the status of a service
    pub async fn status<S: AsRef<str>>(
        &self,
        name: S,
    ) -> Result<crate::zinit::service::ZInitStatus> {
        let table = self.services.read().await;
        let service = table
            .get(name.as_ref())
            .ok_or_else(|| ZInitError::unknown_service(name.as_ref()))?;

        let service = service.read().await.status();
        Ok(service)
    }

    /// Start a service
    pub async fn start<S: AsRef<str>>(&self, name: S) -> Result<()> {
        if *self.shutdown.read().await {
            return Err(ZInitError::ShuttingDown.into());
        }

        self.set_service_state(name.as_ref(), None, Some(Target::Up))
            .await;

        let table = self.services.read().await;
        let service = table
            .get(name.as_ref())
            .ok_or_else(|| ZInitError::unknown_service(name.as_ref()))?;

        let lifecycle = self.clone_lifecycle();
        tokio::spawn(lifecycle.watch_service(name.as_ref().into(), Arc::clone(service)));

        Ok(())
    }

    /// Stop a service
    pub async fn stop<S: AsRef<str>>(&self, name: S) -> Result<()> {
        let table = self.services.read().await;
        let service = table
            .get(name.as_ref())
            .ok_or_else(|| ZInitError::unknown_service(name.as_ref()))?;

        let mut service = service.write().await;
        service.set_target(Target::Down);

        let signal = match signal::Signal::from_str(&service.service.signal.stop.to_uppercase()) {
            Ok(signal) => signal,
            Err(err) => {
                return Err(anyhow::anyhow!(
                    "unknown stop signal configured '{}': {}",
                    service.service.signal.stop,
                    err
                ));
            }
        };

        if service.pid.as_raw() == 0 {
            return Ok(());
        }

        self.pm.signal(service.pid, signal)
    }

    /// Forget a service
    pub async fn forget<S: AsRef<str>>(&self, name: S) -> Result<()> {
        let mut table = self.services.write().await;
        let service = table
            .get(name.as_ref())
            .ok_or_else(|| ZInitError::unknown_service(name.as_ref()))?;

        let service = service.read().await;
        if service.target == Target::Up || service.pid != Pid::from_raw(0) {
            return Err(ZInitError::service_is_up(name.as_ref()).into());
        }

        drop(service);
        table.remove(name.as_ref());

        Ok(())
    }

    /// Send a signal to a service
    pub async fn kill<S: AsRef<str>>(&self, name: S, signal: signal::Signal) -> Result<()> {
        let table = self.services.read().await;
        let service = table
            .get(name.as_ref())
            .ok_or_else(|| ZInitError::unknown_service(name.as_ref()))?;

        let service = service.read().await;
        if service.pid == Pid::from_raw(0) {
            return Err(ZInitError::service_is_down(name.as_ref()).into());
        }

        self.pm.signal(service.pid, signal)
    }

    /// List all services
    pub async fn list(&self) -> Result<Vec<String>> {
        let table = self.services.read().await;
        Ok(table.keys().map(|k| k.into()).collect())
    }

    /// Shutdown the system
    pub async fn shutdown(&self) -> Result<()> {
        info!("shutting down");
        self.power(RebootMode::RB_POWER_OFF).await
    }

    /// Reboot the system
    pub async fn reboot(&self) -> Result<()> {
        info!("rebooting");
        self.power(RebootMode::RB_AUTOBOOT).await
    }

    /// Power off or reboot the system
    async fn power(&self, mode: RebootMode) -> Result<()> {
        *self.shutdown.write().await = true;

        let mut state_channels: HashMap<String, Watcher<State>> = HashMap::new();
        let mut shutdown_timeouts: HashMap<String, u64> = HashMap::new();

        let table = self.services.read().await;
        for (name, service) in table.iter() {
            let service = service.read().await;
            if service.is_active() {
                info!("service '{}' is scheduled for a shutdown", name);
                state_channels.insert(name.into(), service.state_watcher());
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

    /// Kill processes in dependency order
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
                    let lifecycle = self.clone_lifecycle();
                    tokio::spawn(Self::kill_wait(
                        lifecycle,
                        child.to_string(),
                        tx.clone(),
                        watcher.unwrap(),
                        shutdown_timeout.unwrap_or(config::DEFAULT_SHUTDOWN_TIMEOUT),
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

    /// Wait for a service to be killed
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
                    if !state.is_active() {
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

    /// Check if a service can be scheduled
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
                        ps.get_state(),
                        ps.service.one_shot
                    );

                    match ps.get_state() {
                        State::Running if !ps.service.one_shot => true,
                        State::Success => true,
                        _ => false,
                    }
                }
                // depending on an undefined service. This still can be resolved later
                // by monitoring the dependency in the future.
                None => false,
            };

            // if state is blocked, we can break the loop
            if !can {
                break;
            }
        }

        can
    }

    /// Set the state and/or target of a service
    async fn set_service_state(&self, name: &str, state: Option<State>, target: Option<Target>) {
        let table = self.services.read().await;
        let service = match table.get(name) {
            Some(service) => service,
            None => return,
        };

        let mut service = service.write().await;
        if let Some(state) = state {
            service.force_set_state(state);
        }

        if let Some(target) = target {
            service.set_target(target);
        }
    }

    /// Test if a service is running correctly
    async fn test_service<S: AsRef<str>>(&self, name: S, cfg: &config::Service) -> Result<bool> {
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
        Ok(status.success())
    }

    /// Run the test loop for a service
    async fn test_loop(self, name: String, cfg: config::Service) {
        loop {
            let result = self.test_service(&name, &cfg).await;

            match result {
                Ok(result) => {
                    if result {
                        self.set_service_state(&name, Some(State::Running), None)
                            .await;
                        // release
                        self.notify.notify_waiters();
                        return;
                    }
                    // wait before we try again
                    time::sleep(std::time::Duration::from_secs(2)).await;
                }
                Err(_) => {
                    self.set_service_state(&name, Some(State::TestFailure), None)
                        .await;
                }
            }
        }
    }

    /// Watch a service and manage its lifecycle
    async fn watch_service(self, name: String, input: Arc<RwLock<ZInitService>>) {
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
            // are all dependent services are running?

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

                self.set_service_state(&name, Some(State::Blocked), None)
                    .await;
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
                    service.force_set_state(State::Spawned);
                    service.set_pid(child.pid);
                    child
                }
                Err(err) => {
                    // so, spawning failed. and nothing we can do about it
                    // this can be duo to a bad command or exe not found.
                    // set service to failure.
                    error!("service {} failed to start: {}", name, err);
                    service.force_set_state(State::Failure);
                    break;
                }
            };

            if config.one_shot {
                service.force_set_state(State::Running);
            }

            // we don't lock here because this can take forever
            // to finish. so we allow other operation on the service (for example)
            // status and stop operations.
            drop(service);

            let mut handler = None;
            if !config.one_shot {
                let lifecycle = self.clone_lifecycle();
                handler = Some(tokio::spawn(
                    lifecycle.test_loop(name.clone(), config.clone()),
                ));
            }

            let result = child.wait().await;
            if let Some(handler) = handler {
                handler.abort();
            }

            let mut service = input.write().await;
            service.clear_pid();

            match result {
                Err(err) => {
                    error!("failed to read service '{}' status: {}", name, err);
                    service.force_set_state(State::Unknown);
                }
                Ok(status) => {
                    service.force_set_state(if status.success() {
                        State::Success
                    } else {
                        State::Error(status)
                    });
                }
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

    /// Clone the lifecycle manager
    pub fn clone_lifecycle(&self) -> Self {
        Self {
            pm: self.pm.clone(),
            services: Arc::clone(&self.services),
            notify: Arc::clone(&self.notify),
            shutdown: Arc::clone(&self.shutdown),
            container: self.container,
        }
    }
}
