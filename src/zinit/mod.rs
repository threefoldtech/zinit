pub mod config;

use crate::manager::{Log, Logs, Process, ProcessManager};
use anyhow::Result;
use nix::sys::signal;
use nix::sys::wait::WaitStatus;
use nix::unistd::Pid;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{Notify, RwLock};
use tokio::time;

pub trait WaitStatusExt {
    fn success(&self) -> bool;
}

impl WaitStatusExt for WaitStatus {
    fn success(&self) -> bool {
        match *self {
            WaitStatus::Exited(_, code) if code == 0 => true,
            _ => false,
        }
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
#[derive(Clone)]
pub struct ZInitService {
    pub pid: Pid,
    pub state: State,
    // config is the service configuration
    pub service: config::Service,
    // target is the target state of the service (up, down)
    pub target: Target,
    pub scheduled: bool,
}

impl ZInitService {
    fn new(service: config::Service, state: State) -> ZInitService {
        ZInitService {
            pid: Pid::from_raw(0),
            state,
            service: service,
            target: Target::Up,
            scheduled: false,
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

type Table = HashMap<String, ZInitService>;

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
            container: container,
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
            bail!(ZInitError::ServiceAlreadyMonitored { name: name })
        }

        services.insert(name.clone(), ZInitService::new(service, State::Unknown));
        let m = self.clone();
        debug!("service '{}' monitored", name);
        tokio::spawn(m.watch(name));
        Ok(())
    }

    pub async fn status<S: AsRef<str>>(&self, name: S) -> Result<ZInitService> {
        let table = self.services.read().await;
        let service = table.get(name.as_ref());

        let service = match service {
            Some(service) => service,
            None => bail!(ZInitError::UnknownService {
                name: name.as_ref().into()
            }),
        };

        Ok(service.clone())
    }

    pub async fn shutdown(&self) -> Result<()> {
        debug!("shutting down");
        *self.shutdown.write().await = true;
        loop {
            let table = self.services.read().await;
            let mut to_kill: Vec<String> = Vec::new();
            for (name, service) in table.iter() {
                if service.state == State::Running || service.state == State::Spawned {
                    debug!("service '{}' is scheduled for a shutdown", name);
                    to_kill.push(name.into());
                }
            }

            drop(table);

            if to_kill.len() == 0 {
                break;
            }

            for name in to_kill {
                debug!("stopping '{}'", name);
                let _ = self.stop(&name).await;
            }
            // sleep for a second before
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        nix::unistd::sync();
        if self.container {
            std::process::exit(0);
        } else {
            nix::sys::reboot::reboot(nix::sys::reboot::RebootMode::RB_AUTOBOOT)?;
        }
        Ok(())
    }

    pub async fn stop<S: AsRef<str>>(&self, name: S) -> Result<()> {
        let mut table = self.services.write().await;
        let service = table.get_mut(name.as_ref());

        let mut service = match service {
            Some(service) => service,
            None => bail!(ZInitError::UnknownService {
                name: name.as_ref().into()
            }),
        };

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

        if table.get(name.as_ref()).is_none() {
            bail!(ZInitError::UnknownService {
                name: name.as_ref().into()
            })
        }

        let m = self.clone();
        tokio::spawn(m.watch(name.as_ref().into()));
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

        if service.target == Target::Up || service.pid != Pid::from_raw(0) {
            bail!(ZInitError::ServiceISUp {
                name: name.as_ref().into()
            })
        }

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
                    debug!(
                        "- service {} is {:?} oneshot: {}",
                        dep, ps.state, ps.service.one_shot
                    );
                    match ps.state {
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

    async fn test<S: Into<String>>(self, name: S) {
        let name = name.into();
        loop {
            let result = self.test_once(&name).await;

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

    async fn test_once<S: AsRef<str>>(&self, name: S) -> Result<bool> {
        let table = self.services.read().await;
        let service = match table.get(name.as_ref()) {
            Some(service) => service,
            None => bail!("service not found"),
        };

        if service.state != State::Spawned {
            return Ok(true);
        }

        let cfg = service.service.clone();
        drop(table);

        if cfg.test.len() == 0 {
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

        return Ok(false);
    }

    async fn set(
        &self,
        name: &str,
        state: Option<State>,
        target: Option<Target>,
        scheduled: Option<bool>,
    ) {
        let mut table = self.services.write().await;
        let service = match table.get_mut(name) {
            Some(service) => service,
            None => return,
        };

        if let Some(state) = state {
            service.state = state;
        }

        if let Some(target) = target {
            service.target = target;
        }

        if let Some(scheduled) = scheduled {
            service.scheduled = scheduled
        }
    }

    async fn watch(self, name: String) {
        let name = name.clone();
        let mut table = self.services.write().await;

        let mut service = match table.get_mut(&name) {
            Some(ps) => ps,
            None => {
                // process has been forgotten
                return;
            }
        };

        if service.target == Target::Down {
            debug!("service '{}' target is down", name);
            return;
        }

        if service.scheduled {
            debug!("service '{}' already scheduled", name);
            return;
        }

        service.scheduled = true;
        drop(table);

        loop {
            let name = name.clone();
            let table = self.services.read().await;

            let service = match table.get(&name) {
                Some(ps) => ps,
                None => {
                    // process has been forgotten
                    break;
                }
            };

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
            drop(table);

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

            let mut table = self.services.write().await;
            let mut service = match table.get_mut(&name) {
                Some(service) => service,
                None => {
                    // service is gone, we need to make sure child is gone as well
                    if let Ok(child) = child {
                        let _ = child.signal(signal::Signal::SIGKILL);
                    }
                    break;
                }
            };

            let child = match child {
                Ok(child) => {
                    service.state = State::Spawned;
                    service.pid = child.pid;
                    child
                }
                Err(err) => {
                    // so, spawning failed. and nothing we can do about it
                    // this can be duo to a bad command or exe not found.
                    // set service to failure.
                    error!("service {} failed to start: {}", name, err);
                    service.state = State::Failure;
                    break;
                }
            };

            if config.one_shot {
                service.state = State::Running;
            }
            // we don't lock the table here because this can take forever
            // to finish. so we allow other services to schedule
            drop(table);
            let mut handler = None;
            if !config.one_shot {
                let m = self.clone();
                handler = Some(tokio::spawn(m.test(name.clone())));
            }

            let result = child.wait().await;
            if let Some(handler) = handler {
                handler.abort();
            }

            let mut table = self.services.write().await;
            let mut service = match table.get_mut(&name) {
                Some(service) => service,
                None => {
                    break;
                }
            };

            service.pid = Pid::from_raw(0);
            match result {
                Err(err) => {
                    error!("failed to read service '{}' status: {}", name, err);
                    service.state = State::Unknown;
                }
                Ok(status) => {
                    service.state = match status.success() {
                        true => State::Success,
                        false => State::Error(status),
                    }
                }
            };

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
