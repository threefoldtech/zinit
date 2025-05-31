use crate::manager::{Log, Logs, Process, ProcessManager};
use crate::zinit::config;
use crate::zinit::errors::ZInitError;
#[cfg(target_os = "linux")]
use crate::zinit::ord::{service_dependency_order, ProcessDAG, DUMMY_ROOT};
use crate::zinit::service::ZInitService;
use crate::zinit::state::{State, Target};
#[cfg(target_os = "linux")]
use crate::zinit::types::Watcher;
use crate::zinit::types::{ProcessStats, ServiceStats, ServiceTable};
use std::collections::HashMap;
use sysinfo::{self, PidExt, ProcessExt, System, SystemExt};

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
#[cfg(target_os = "linux")]
use nix::sys::reboot::RebootMode;
use nix::sys::signal;
use nix::sys::wait::WaitStatus;
use nix::unistd::Pid;
use std::str::FromStr;
use std::sync::Arc;
#[cfg(target_os = "linux")]
use tokio::sync::mpsc;
use tokio::sync::{Notify, RwLock};
use tokio::time::sleep;
#[cfg(target_os = "linux")]
use tokio::time::timeout;
#[cfg(target_os = "linux")]
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
    pub async fn logs(&self, existing_logs: bool, follow: bool) -> Logs {
        self.pm.stream(existing_logs, follow).await
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
        // Get service information
        let table = self.services.read().await;
        let service = table
            .get(name.as_ref())
            .ok_or_else(|| ZInitError::unknown_service(name.as_ref()))?;

        let mut service = service.write().await;
        service.set_target(Target::Down);

        // Get the main process PID
        let pid = service.pid;
        if pid.as_raw() == 0 {
            return Ok(());
        }

        // Get the signal to use
        let signal = signal::Signal::from_str(&service.service.signal.stop.to_uppercase())
            .map_err(|err| anyhow::anyhow!("unknown stop signal: {}", err))?;

        // Release the lock before potentially long-running operations
        drop(service);
        drop(table);

        // Get all child processes using our stats functionality
        let children = self.get_child_process_stats(pid.as_raw()).await?;

        // First try to stop the process group
        let _ = self.pm.signal(pid, signal);

        // Wait a short time for processes to terminate gracefully
        sleep(std::time::Duration::from_millis(500)).await;

        // Check if processes are still running and use SIGKILL if needed
        self.ensure_processes_terminated(pid.as_raw(), &children)
            .await?;

        Ok(())
    }

    /// Ensure that a process and its children are terminated
    async fn ensure_processes_terminated(
        &self,
        parent_pid: i32,
        children: &[ProcessStats],
    ) -> Result<()> {
        // Check if parent is still running
        let parent_running = self.is_process_running(parent_pid).await?;

        // If parent is still running, send SIGKILL
        if parent_running {
            debug!(
                "Process {} still running after SIGTERM, sending SIGKILL",
                parent_pid
            );
            let _ = self
                .pm
                .signal(Pid::from_raw(parent_pid), signal::Signal::SIGKILL);
        }

        // Check and kill any remaining child processes
        for child in children {
            if self.is_process_running(child.pid).await? {
                debug!("Child process {} still running, sending SIGKILL", child.pid);
                let _ = signal::kill(Pid::from_raw(child.pid), signal::Signal::SIGKILL);
            }
        }

        // Verify all processes are gone
        let mut retries = 5;
        while retries > 0 {
            let mut all_terminated = true;

            // Check parent
            if self.is_process_running(parent_pid).await? {
                all_terminated = false;
            }

            // Check children
            for child in children {
                if self.is_process_running(child.pid).await? {
                    all_terminated = false;
                    break;
                }
            }

            if all_terminated {
                return Ok(());
            }

            // Wait before retrying
            sleep(std::time::Duration::from_millis(100)).await;
            retries -= 1;
        }

        // If we get here, some processes might still be running
        warn!("Some processes may still be running after shutdown attempts");
        Ok(())
    }

    /// Check if a process is running
    async fn is_process_running(&self, pid: i32) -> Result<bool> {
        // Use sysinfo to check if process exists
        let mut system = System::new();
        let sys_pid = sysinfo::Pid::from(pid as usize);
        system.refresh_process(sys_pid);

        Ok(system.process(sys_pid).is_some())
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

    /// Get stats for a service (memory and CPU usage)
    pub async fn stats<S: AsRef<str>>(&self, name: S) -> Result<ServiceStats> {
        let table = self.services.read().await;
        let service = table
            .get(name.as_ref())
            .ok_or_else(|| ZInitError::unknown_service(name.as_ref()))?;

        let service = service.read().await;
        if service.pid.as_raw() == 0 {
            return Err(ZInitError::service_is_down(name.as_ref()).into());
        }

        // Get stats for the main process
        let pid = service.pid.as_raw();
        let (memory_usage, cpu_usage) = self.get_process_stats(pid).await?;

        // Get stats for child processes
        let children = self.get_child_process_stats(pid).await?;

        Ok(ServiceStats {
            memory_usage,
            cpu_usage,
            pid,
            children,
        })
    }

    /// Get memory and CPU usage for a process
    async fn get_process_stats(&self, pid: i32) -> Result<(u64, f32)> {
        // Create a new System instance with all information
        let mut system = System::new_all();
        
        // Convert i32 pid to sysinfo::Pid
        let sys_pid = sysinfo::Pid::from(pid as usize);
        
        // Make sure we're refreshing CPU information
        system.refresh_cpu();
        system.refresh_processes();
        
        // First refresh to get initial CPU values
        system.refresh_all();
        
        // Wait longer for CPU measurement (500ms instead of 100ms)
        sleep(std::time::Duration::from_millis(500)).await;
        
        // Refresh again to get updated CPU values
        system.refresh_cpu();
        system.refresh_processes();
        system.refresh_all();
        
        // Get the process
        if let Some(process) = system.process(sys_pid) {
            // Get memory in bytes
            let memory_usage = process.memory();
            
            // Get CPU usage as percentage
            let cpu_usage = process.cpu_usage();
            
            Ok((memory_usage, cpu_usage))
        } else {
            // Process not found
            Ok((0, 0.0))
        }
    }

    /// Get stats for child processes
    async fn get_child_process_stats(&self, parent_pid: i32) -> Result<Vec<ProcessStats>> {
        // Create a new System instance with all processes information
        let mut system = System::new_all();
        
        // Make sure we're refreshing CPU information
        system.refresh_cpu();
        system.refresh_processes();
        system.refresh_all();
        
        // Convert i32 pid to sysinfo::Pid
        let sys_pid = sysinfo::Pid::from(parent_pid as usize);
        
        // Wait longer for CPU measurement (500ms instead of 100ms)
        sleep(std::time::Duration::from_millis(500)).await;
        
        // Refresh all system information to get updated CPU values
        system.refresh_cpu();
        system.refresh_processes();
        system.refresh_all();

        let mut children = Vec::new();

        // Recursively collect all descendant PIDs
        let mut descendant_pids = Vec::new();
        self.collect_descendants(sys_pid, &system.processes(), &mut descendant_pids);

        // Get stats for each child process
        for &child_pid in &descendant_pids {
            if let Some(process) = system.process(child_pid) {
                children.push(ProcessStats {
                    pid: child_pid.as_u32() as i32,
                    memory_usage: process.memory(),
                    cpu_usage: process.cpu_usage(),
                });
            }
        }

        Ok(children)
    }

    /// Recursively collect all descendant PIDs of a process
    fn collect_descendants(
        &self,
        pid: sysinfo::Pid,
        procs: &HashMap<sysinfo::Pid, sysinfo::Process>,
        out: &mut Vec<sysinfo::Pid>,
    ) {
        for (&child_pid, proc) in procs.iter() {
            if proc.parent() == Some(pid) {
                out.push(child_pid);
                self.collect_descendants(child_pid, procs, out);
            }
        }
    }

    /// Verify that all processes are terminated
    async fn verify_all_processes_terminated(&self) -> Result<()> {
        // Get all services
        let table = self.services.read().await;

        // Check each service
        for (name, service) in table.iter() {
            let service = service.read().await;
            let pid = service.pid.as_raw();

            // Skip services with no PID
            if pid == 0 {
                continue;
            }

            // Check if the main process is still running
            if self.is_process_running(pid).await? {
                warn!(
                    "Service {} (PID {}) is still running after shutdown",
                    name, pid
                );

                // Try to kill it with SIGKILL
                let _ = signal::kill(Pid::from_raw(pid), signal::Signal::SIGKILL);
            }

            // Check for child processes
            if let Ok(children) = self.get_child_process_stats(pid).await {
                for child in children {
                    if self.is_process_running(child.pid).await? {
                        warn!(
                            "Child process {} of service {} is still running after shutdown",
                            child.pid, name
                        );

                        // Try to kill it with SIGKILL
                        let _ = signal::kill(Pid::from_raw(child.pid), signal::Signal::SIGKILL);
                    }
                }
            }
        }

        Ok(())
    }

    /// Shutdown the system
    pub async fn shutdown(&self) -> Result<()> {
        info!("shutting down");

        // Set the shutdown flag
        *self.shutdown.write().await = true;

        #[cfg(target_os = "linux")]
        {
            // Power off using our enhanced method
            let result = self.power(RebootMode::RB_POWER_OFF).await;

            // Final verification before exit
            self.verify_all_processes_terminated().await?;

            return result;
        }

        #[cfg(not(target_os = "linux"))]
        {
            // Stop all services
            let services = self.list().await?;
            for service in services {
                let _ = self.stop(&service).await;
            }

            // Verify all processes are terminated
            self.verify_all_processes_terminated().await?;

            if self.container {
                std::process::exit(0);
            } else {
                info!("System shutdown not supported on this platform");
                std::process::exit(0);
            }
        }
    }

    /// Reboot the system
    pub async fn reboot(&self) -> Result<()> {
        info!("rebooting");

        // Set the shutdown flag
        *self.shutdown.write().await = true;

        #[cfg(target_os = "linux")]
        {
            // Reboot using our enhanced method
            let result = self.power(RebootMode::RB_AUTOBOOT).await;

            // Final verification before exit
            self.verify_all_processes_terminated().await?;

            return result;
        }

        #[cfg(not(target_os = "linux"))]
        {
            // Stop all services
            let services = self.list().await?;
            for service in services {
                let _ = self.stop(&service).await;
            }

            // Verify all processes are terminated
            self.verify_all_processes_terminated().await?;

            if self.container {
                std::process::exit(0);
            } else {
                info!("System reboot not supported on this platform");
                std::process::exit(0);
            }
        }
    }

    /// Power off or reboot the system
    #[cfg(target_os = "linux")]
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

        // On Linux, we can use sync and reboot
        nix::unistd::sync();
        if self.container {
            std::process::exit(0);
        } else {
            nix::sys::reboot::reboot(mode)?;
        }

        Ok(())
    }

    /// Kill processes in dependency order
    #[cfg(target_os = "linux")]
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

                    // Spawn a task to kill the service and wait for it to terminate
                    let kill_task = tokio::spawn(Self::kill_wait_enhanced(
                        lifecycle,
                        child.to_string(),
                        tx.clone(),
                        watcher.unwrap(),
                        shutdown_timeout.unwrap_or(config::DEFAULT_SHUTDOWN_TIMEOUT),
                    ));

                    // Add a timeout to ensure we don't wait forever
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(
                            shutdown_timeout.unwrap_or(config::DEFAULT_SHUTDOWN_TIMEOUT) + 2,
                        ),
                        kill_task,
                    )
                    .await;
                }
            }

            count -= 1;
            if count == 0 {
                break;
            }
        }

        // Final verification that all processes are gone
        self.verify_all_processes_terminated().await?;

        Ok(())
    }

    /// Enhanced version of kill_wait that ensures processes are terminated
    #[cfg(target_os = "linux")]
    async fn kill_wait_enhanced(
        self,
        name: String,
        ch: mpsc::UnboundedSender<String>,
        mut rx: Watcher<State>,
        shutdown_timeout: u64,
    ) {
        debug!("kill_wait {}", name);

        // Try to stop the service gracefully
        let stop_result = self.stop(name.clone()).await;

        // Wait for the service to become inactive or timeout
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

        match stop_result {
            Ok(_) => {
                let _ = fut.await;
            }
            Err(e) => error!("couldn't stop service {}: {}", name.clone(), e),
        }

        // Verify the service is actually stopped
        if let Ok(status) = self.status(&name).await {
            if status.pid != Pid::from_raw(0) {
                // Service is still running, try to kill it
                let _ = self.kill(&name, signal::Signal::SIGKILL).await;
            }
        }

        debug!("sending to the death channel {}", name.clone());
        if let Err(e) = ch.send(name.clone()) {
            error!(
                "error: couldn't send the service {} to the shutdown loop: {}",
                name, e
            );
        }
    }

    /// Original kill_wait for backward compatibility
    #[cfg(target_os = "linux")]
    async fn kill_wait(
        self,
        name: String,
        ch: mpsc::UnboundedSender<String>,
        rx: Watcher<State>,
        shutdown_timeout: u64,
    ) {
        Self::kill_wait_enhanced(self, name, ch, rx, shutdown_timeout).await
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
                    sleep(std::time::Duration::from_secs(2)).await;
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
            sleep(std::time::Duration::from_secs(2)).await;
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
