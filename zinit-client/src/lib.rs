//! A client library for interacting with the Zinit process manager.
//!
//! This library provides a simple API for communicating with a Zinit daemon
//! via either Unix socket (using reth-ipc) or HTTP (using jsonrpsee).
use jsonrpsee::core::client::ClientT;
use jsonrpsee::core::client::Error as RpcError;
use jsonrpsee::http_client::{HttpClient, HttpClientBuilder};
use jsonrpsee::rpc_params;
use reth_ipc::client::IpcClientBuilder;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use thiserror::Error;

/// Error type for client operations
#[derive(Error, Debug)]
pub enum ClientError {
    #[error("connection error: {0}")]
    ConnectionError(String),

    #[error("service not found: {0}")]
    ServiceNotFound(String),

    #[error("service already monitored: {0}")]
    ServiceAlreadyMonitored(String),

    #[error("service is already up: {0}")]
    ServiceIsUp(String),

    #[error("service is down: {0}")]
    ServiceIsDown(String),

    #[error("invalid signal: {0}")]
    InvalidSignal(String),

    #[error("config error: {0}")]
    ConfigError(String),

    #[error("system is shutting down")]
    ShuttingDown,

    #[error("service already exists: {0}")]
    ServiceAlreadyExists(String),

    #[error("service file error: {0}")]
    ServiceFileError(String),

    #[error("rpc error: {0}")]
    RpcError(String),

    #[error("unknown error: {0}")]
    UnknownError(String),
}

impl From<RpcError> for ClientError {
    fn from(err: RpcError) -> Self {
        if let RpcError::Call(call) = &err {
            let code = call.code();
            let message = call.message().to_string();

            // Try to parse structured data payload if present
            let details: Option<serde_json::Value> = call
                .data()
                .and_then(|raw| serde_json::from_str(raw.get()).ok());

            let code_name = details
                .as_ref()
                .and_then(|d| d.get("code_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");

            let service = details
                .as_ref()
                .and_then(|d| d.get("service"))
                .and_then(|v| v.as_str());

            let action = details
                .as_ref()
                .and_then(|d| d.get("action"))
                .and_then(|v| v.as_str());

            let hint = details
                .as_ref()
                .and_then(|d| d.get("hint"))
                .and_then(|v| v.as_str());

            let chain = details.as_ref().and_then(|d| d.get("cause_chain")).cloned();

            let human = match (service, action, hint) {
                (Some(s), Some(a), Some(h)) => {
                    format!(
                        "{}[{}]: {} while {} '{}'. Hint: {}. Details: {:?}",
                        code_name, code, message, a, s, h, chain
                    )
                }
                (Some(s), Some(a), None) => {
                    format!(
                        "{}[{}]: {} while {} '{}'. Details: {:?}",
                        code_name, code, message, a, s, chain
                    )
                }
                _ => {
                    format!("{}[{}]: {}. Details: {:?}", code_name, code, message, chain)
                }
            };

            return match code {
                -32000 => ClientError::ServiceNotFound(human),
                -32001 => ClientError::ServiceAlreadyMonitored(human),
                -32002 => ClientError::ServiceIsUp(human),
                -32003 => ClientError::ServiceIsDown(human),
                -32004 => ClientError::InvalidSignal(human),
                -32005 => ClientError::ConfigError(human),
                -32006 => ClientError::ShuttingDown,
                -32007 => ClientError::ServiceAlreadyExists(human),
                -32008 => ClientError::ServiceFileError(human),
                _ => ClientError::RpcError(human),
            };
        }

        match err {
            RpcError::Transport(_) => ClientError::ConnectionError(err.to_string()),
            _ => ClientError::RpcError(err.to_string()),
        }
    }
}

/// Service status information
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Status {
    pub name: String,
    pub pid: u32,
    pub state: String,
    pub target: String,
    pub after: HashMap<String, String>,
}

/// Child process stats information
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChildStats {
    pub pid: u32,
    pub memory_usage: u64,
    pub cpu_usage: f32,
}

/// Service stats information
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Stats {
    pub name: String,
    pub pid: u32,
    pub memory_usage: u64,
    pub cpu_usage: f32,
    pub children: Vec<ChildStats>,
}

/// Client implementation for communicating with Zinit
pub enum Client {
    Ipc(String), // Socket path
    Http(HttpClient),
}

impl Client {
    /// Create a new client using Unix socket transport
    pub async fn unix_socket<P: AsRef<std::path::Path>>(path: P) -> Result<Self, ClientError> {
        Ok(Client::Ipc(path.as_ref().to_string_lossy().to_string()))
    }

    /// Create a new client using HTTP transport
    pub async fn http<S: AsRef<str>>(url: S) -> Result<Self, ClientError> {
        let client = HttpClientBuilder::default()
            .build(url.as_ref())
            .map_err(|e| ClientError::ConnectionError(e.to_string()))?;

        Ok(Client::Http(client))
    }

    // Helper to get IPC client
    async fn get_ipc_client(&self) -> Result<impl ClientT, ClientError> {
        match self {
            Client::Ipc(path) => IpcClientBuilder::default()
                .build(path)
                .await
                .map_err(|e| ClientError::ConnectionError(e.to_string())),
            _ => Err(ClientError::UnknownError("Not an IPC client".to_string())),
        }
    }

    // Service API Methods

    /// List all monitored services and their current state
    pub async fn list(&self) -> Result<HashMap<String, String>, ClientError> {
        match self {
            Client::Ipc(_) => {
                let client = self.get_ipc_client().await?;
                client
                    .request("service_list", rpc_params![])
                    .await
                    .map_err(Into::into)
            }
            Client::Http(client) => client
                .request("service_list", rpc_params![])
                .await
                .map_err(Into::into),
        }
    }

    /// Get the detailed status of a specific service
    pub async fn status(&self, name: impl AsRef<str>) -> Result<Status, ClientError> {
        let name = name.as_ref().to_string();
        match self {
            Client::Ipc(_) => {
                let client = self.get_ipc_client().await?;
                client
                    .request("service_status", rpc_params![name])
                    .await
                    .map_err(Into::into)
            }
            Client::Http(client) => client
                .request("service_status", rpc_params![name])
                .await
                .map_err(Into::into),
        }
    }

    /// Start a specific service
    pub async fn start(&self, name: impl AsRef<str>) -> Result<(), ClientError> {
        let name = name.as_ref().to_string();
        match self {
            Client::Ipc(_) => {
                let client = self.get_ipc_client().await?;
                client
                    .request("service_start", rpc_params![name])
                    .await
                    .map_err(Into::into)
            }
            Client::Http(client) => client
                .request("service_start", rpc_params![name])
                .await
                .map_err(Into::into),
        }
    }

    /// Stop a specific service
    pub async fn stop(&self, name: impl AsRef<str>) -> Result<(), ClientError> {
        let name = name.as_ref().to_string();
        match self {
            Client::Ipc(_) => {
                let client = self.get_ipc_client().await?;
                client
                    .request("service_stop", rpc_params![name])
                    .await
                    .map_err(Into::into)
            }
            Client::Http(client) => client
                .request("service_stop", rpc_params![name])
                .await
                .map_err(Into::into),
        }
    }

    /// Restart a service
    pub async fn restart(&self, name: impl AsRef<str>) -> Result<(), ClientError> {
        let name = name.as_ref().to_string();
        // First stop the service
        self.stop(&name).await?;

        // Poll the service status until it's stopped
        for _ in 0..20 {
            let status = self.status(&name).await?;
            if status.pid == 0 && status.target == "Down" {
                return self.start(&name).await;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        // Process not stopped, try to kill it
        self.kill(&name, "SIGKILL").await?;
        self.start(&name).await
    }

    /// Load and monitor a new service from its configuration file
    pub async fn monitor(&self, name: impl AsRef<str>) -> Result<(), ClientError> {
        let name = name.as_ref().to_string();
        match self {
            Client::Ipc(_) => {
                let client = self.get_ipc_client().await?;
                client
                    .request("service_monitor", rpc_params![name])
                    .await
                    .map_err(Into::into)
            }
            Client::Http(client) => client
                .request("service_monitor", rpc_params![name])
                .await
                .map_err(Into::into),
        }
    }

    /// Stop monitoring a service and remove it from management
    pub async fn forget(&self, name: impl AsRef<str>) -> Result<(), ClientError> {
        let name = name.as_ref().to_string();
        match self {
            Client::Ipc(_) => {
                let client = self.get_ipc_client().await?;
                client
                    .request("service_forget", rpc_params![name])
                    .await
                    .map_err(Into::into)
            }
            Client::Http(client) => client
                .request("service_forget", rpc_params![name])
                .await
                .map_err(Into::into),
        }
    }

    /// Send a signal to a specific service process
    pub async fn kill(
        &self,
        name: impl AsRef<str>,
        signal: impl AsRef<str>,
    ) -> Result<(), ClientError> {
        let name = name.as_ref().to_string();
        let signal = signal.as_ref().to_string();
        match self {
            Client::Ipc(_) => {
                let client = self.get_ipc_client().await?;
                client
                    .request("service_kill", rpc_params![name, signal])
                    .await
                    .map_err(Into::into)
            }
            Client::Http(client) => client
                .request("service_kill", rpc_params![name, signal])
                .await
                .map_err(Into::into),
        }
    }

    /// Create a new service configuration
    pub async fn create_service(
        &self,
        name: impl AsRef<str>,
        content: Map<String, Value>,
    ) -> Result<String, ClientError> {
        let name = name.as_ref().to_string();
        match self {
            Client::Ipc(_) => {
                let client = self.get_ipc_client().await?;
                client
                    .request("service_create", rpc_params![name, content])
                    .await
                    .map_err(Into::into)
            }
            Client::Http(client) => client
                .request("service_create", rpc_params![name, content])
                .await
                .map_err(Into::into),
        }
    }

    /// Delete a service configuration
    pub async fn delete_service(&self, name: impl AsRef<str>) -> Result<String, ClientError> {
        let name = name.as_ref().to_string();
        match self {
            Client::Ipc(_) => {
                let client = self.get_ipc_client().await?;
                client
                    .request("service_delete", rpc_params![name])
                    .await
                    .map_err(Into::into)
            }
            Client::Http(client) => client
                .request("service_delete", rpc_params![name])
                .await
                .map_err(Into::into),
        }
    }

    /// Get a service configuration
    pub async fn get_service(&self, name: impl AsRef<str>) -> Result<Value, ClientError> {
        let name = name.as_ref().to_string();
        match self {
            Client::Ipc(_) => {
                let client = self.get_ipc_client().await?;
                client
                    .request("service_get", rpc_params![name])
                    .await
                    .map_err(Into::into)
            }
            Client::Http(client) => client
                .request("service_get", rpc_params![name])
                .await
                .map_err(Into::into),
        }
    }

    /// Get memory and CPU usage statistics for a service
    pub async fn stats(&self, name: impl AsRef<str>) -> Result<Stats, ClientError> {
        let name = name.as_ref().to_string();
        match self {
            Client::Ipc(_) => {
                let client = self.get_ipc_client().await?;
                client
                    .request("service_stats", rpc_params![name])
                    .await
                    .map_err(Into::into)
            }
            Client::Http(client) => client
                .request("service_stats", rpc_params![name])
                .await
                .map_err(Into::into),
        }
    }

    // System API Methods

    /// Initiate system shutdown
    pub async fn shutdown(&self) -> Result<(), ClientError> {
        match self {
            Client::Ipc(_) => {
                let client = self.get_ipc_client().await?;
                client
                    .request("system_shutdown", rpc_params![])
                    .await
                    .map_err(Into::into)
            }
            Client::Http(client) => client
                .request("system_shutdown", rpc_params![])
                .await
                .map_err(Into::into),
        }
    }

    /// Initiate system reboot
    pub async fn reboot(&self) -> Result<(), ClientError> {
        match self {
            Client::Ipc(_) => {
                let client = self.get_ipc_client().await?;
                client
                    .request("system_reboot", rpc_params![])
                    .await
                    .map_err(Into::into)
            }
            Client::Http(client) => client
                .request("system_reboot", rpc_params![])
                .await
                .map_err(Into::into),
        }
    }

    /// Start HTTP/RPC server
    pub async fn start_http_server(&self, address: impl AsRef<str>) -> Result<String, ClientError> {
        let address = address.as_ref().to_string();
        match self {
            Client::Ipc(_) => {
                let client = self.get_ipc_client().await?;
                client
                    .request("system_start_http_server", rpc_params![address])
                    .await
                    .map_err(Into::into)
            }
            Client::Http(client) => client
                .request("system_start_http_server", rpc_params![address])
                .await
                .map_err(Into::into),
        }
    }

    /// Stop HTTP/RPC server
    pub async fn stop_http_server(&self) -> Result<(), ClientError> {
        match self {
            Client::Ipc(_) => {
                let client = self.get_ipc_client().await?;
                client
                    .request("system_stop_http_server", rpc_params![])
                    .await
                    .map_err(Into::into)
            }
            Client::Http(client) => client
                .request("system_stop_http_server", rpc_params![])
                .await
                .map_err(Into::into),
        }
    }

    // Logging API Methods

    /// Get current logs
    pub async fn logs(&self, filter: Option<String>) -> Result<Vec<String>, ClientError> {
        match self {
            Client::Ipc(_) => {
                let client = self.get_ipc_client().await?;
                client
                    .request("stream_currentLogs", rpc_params![filter])
                    .await
                    .map_err(Into::into)
            }
            Client::Http(client) => client
                .request("stream_currentLogs", rpc_params![filter])
                .await
                .map_err(Into::into),
        }
    }

    /// Subscribe to logs
    ///
    /// Note: This method is not fully implemented yet. For now, it will return an error.
    pub async fn log_subscribe(&self, _filter: Option<String>) -> Result<(), ClientError> {
        Err(ClientError::UnknownError(
            "Log subscription not implemented yet".to_string(),
        ))
    }
}
