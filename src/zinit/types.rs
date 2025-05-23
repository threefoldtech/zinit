use nix::sys::wait::WaitStatus;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::watch;
use tokio::sync::RwLock;
use tokio_stream::wrappers::WatchStream;

/// Stats information for a service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStats {
    /// Memory usage in bytes
    pub memory_usage: u64,

    /// CPU usage as a percentage (0-100)
    pub cpu_usage: f32,

    /// Process ID of the service
    pub pid: i32,

    /// Child process stats if any
    pub children: Vec<ProcessStats>,
}

/// Stats for an individual process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessStats {
    /// Process ID
    pub pid: i32,

    /// Memory usage in bytes
    pub memory_usage: u64,

    /// CPU usage as a percentage (0-100)
    pub cpu_usage: f32,
}

/// Extension trait for WaitStatus to check if a process exited successfully
pub trait WaitStatusExt {
    fn success(&self) -> bool;
}

impl WaitStatusExt for WaitStatus {
    fn success(&self) -> bool {
        matches!(self, WaitStatus::Exited(_, code) if *code == 0)
    }
}

/// Type alias for a service table mapping service names to service instances
pub type ServiceTable = HashMap<String, Arc<RwLock<crate::zinit::service::ZInitService>>>;

/// Type alias for a watch stream
pub type Watcher<T> = WatchStream<Arc<T>>;

/// A wrapper around a value that can be watched for changes
pub struct Watched<T> {
    v: Arc<T>,
    tx: watch::Sender<Arc<T>>,
}

impl<T> Watched<T>
where
    T: Send + Sync + 'static,
{
    /// Create a new watched value
    pub fn new(v: T) -> Self {
        let v = Arc::new(v);
        let (tx, _) = watch::channel(Arc::clone(&v));
        Self { v, tx }
    }

    /// Set the value and notify watchers
    pub fn set(&mut self, v: T) {
        let v = Arc::new(v);
        self.v = Arc::clone(&v);
        // update the value even when there are no receivers
        self.tx.send_replace(v);
    }

    /// Get a reference to the current value
    pub fn get(&self) -> &T {
        &self.v
    }

    /// Create a watcher for this value
    pub fn watcher(&self) -> Watcher<T> {
        WatchStream::new(self.tx.subscribe())
    }
}
