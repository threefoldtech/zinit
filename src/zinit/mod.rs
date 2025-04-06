pub mod config;
pub mod errors;
pub mod lifecycle;
pub mod ord;
pub mod service;
pub mod state;
pub mod stats;
pub mod types;

// Re-export commonly used items
pub use service::ZInitStatus;
pub use state::State;

use crate::manager::{Logs, ProcessManager};
use anyhow::Result;
use nix::sys::signal;
use std::sync::Arc;
use tokio::sync::{Notify, RwLock};

/// Main ZInit service manager
#[derive(Clone)]
pub struct ZInit {
    /// Lifecycle manager for service management
    lifecycle: lifecycle::LifecycleManager,
}

impl ZInit {
    /// Create a new ZInit instance
    pub fn new(cap: usize, container: bool) -> ZInit {
        let pm = ProcessManager::new(cap);
        let services = Arc::new(RwLock::new(types::ServiceTable::new()));
        let notify = Arc::new(Notify::new());
        let shutdown = Arc::new(RwLock::new(false));

        let lifecycle = lifecycle::LifecycleManager::new(pm, services, notify, shutdown, container);

        ZInit { lifecycle }
    }

    /// Start the service manager
    pub fn serve(&self) {
        self.lifecycle.process_manager().start();
        if self.lifecycle.is_container_mode() {
            let lifecycle = self.lifecycle.clone();
            tokio::spawn(async move {
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
                let _ = lifecycle.shutdown().await;
            });
        }
    }

    /// Get logs from the process manager
    pub async fn logs(&self, follow: bool) -> Logs {
        self.lifecycle.logs(follow).await
    }

    /// Monitor a service
    pub async fn monitor<S: Into<String>>(&self, name: S, service: config::Service) -> Result<()> {
        self.lifecycle.monitor(name, service).await
    }

    /// Get the status of a service
    pub async fn status<S: AsRef<str>>(&self, name: S) -> Result<ZInitStatus> {
        self.lifecycle.status(name).await
    }

    /// Start a service
    pub async fn start<S: AsRef<str>>(&self, name: S) -> Result<()> {
        self.lifecycle.start(name).await
    }

    /// Stop a service
    pub async fn stop<S: AsRef<str>>(&self, name: S) -> Result<()> {
        self.lifecycle.stop(name).await
    }

    /// Forget a service
    pub async fn forget<S: AsRef<str>>(&self, name: S) -> Result<()> {
        self.lifecycle.forget(name).await
    }

    /// Send a signal to a service
    pub async fn kill<S: AsRef<str>>(&self, name: S, signal: signal::Signal) -> Result<()> {
        self.lifecycle.kill(name, signal).await
    }

    /// List all services
    pub async fn list(&self) -> Result<Vec<String>> {
        self.lifecycle.list().await
    }

    /// Shutdown the system
    pub async fn shutdown(&self) -> Result<()> {
        self.lifecycle.shutdown().await
    }

    /// Reboot the system
    pub async fn reboot(&self) -> Result<()> {
        self.lifecycle.reboot().await
    }
}
