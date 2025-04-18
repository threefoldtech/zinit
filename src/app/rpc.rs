use crate::zinit::config;
use crate::{app::api::Status, zinit};
use async_trait::async_trait;
use jsonrpsee::core::RpcResult;
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::types::{ErrorCode, ErrorObject, ErrorObjectOwned};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::str::FromStr;

use super::api::Api;

// Standard JSON-RPC error codes
const INVALID_REQUEST: i32 = -32600;
const METHOD_NOT_FOUND: i32 = -32601;
const INVALID_PARAMS: i32 = -32602;
const INTERNAL_ERROR: i32 = -32603;

// Custom error codes for Zinit
const SERVICE_NOT_FOUND: i32 = -32000;
const SERVICE_ALREADY_MONITORED: i32 = -32001;
const SERVICE_IS_UP: i32 = -32002;
const SERVICE_IS_DOWN: i32 = -32003;
const INVALID_SIGNAL: i32 = -32004;
const CONFIG_ERROR: i32 = -32005;
const SHUTTING_DOWN: i32 = -32006;
const SERVICE_ALREADY_EXISTS: i32 = -32007;
const SERVICE_FILE_ERROR: i32 = -32008;

// Include the OpenRPC specification
const OPENRPC_SPEC: &str = include_str!("../../openrpc.json");

/// RPC methods for discovery.
#[rpc(server, client, namespace = "rpc")]
pub trait ZinitRpcApi {
    /// Returns the OpenRPC specification as a JSON Value.
    #[method(name = "discover")]
    async fn discover(&self) -> RpcResult<Value>;
}

#[async_trait]
impl ZinitRpcApiServer for Api {
    async fn discover(&self) -> RpcResult<Value> {
        let spec = serde_json::from_str(OPENRPC_SPEC).expect("Failed to parse OpenRPC spec");
        Ok(spec)
    }
}

/// RPC methods for service management.
#[rpc(server, client, namespace = "service")]
pub trait ZinitServiceApi {
    /// List all monitored services and their current state.
    /// Returns a map where keys are service names and values are state strings.
    #[method(name = "list")]
    async fn list(&self) -> RpcResult<HashMap<String, String>>;

    /// Get the detailed status of a specific service.
    #[method(name = "status")]
    async fn status(&self, name: String) -> RpcResult<Status>;

    /// Start a specific service.
    #[method(name = "start")]
    async fn start(&self, name: String) -> RpcResult<()>;

    /// Stop a specific service.
    #[method(name = "stop")]
    async fn stop(&self, name: String) -> RpcResult<()>;

    /// Load and monitor a new service from its configuration file (e.g., "service_name.yaml").
    #[method(name = "monitor")]
    async fn monitor(&self, name: String) -> RpcResult<()>;

    /// Stop monitoring a service and remove it from management.
    #[method(name = "forget")]
    async fn forget(&self, name: String) -> RpcResult<()>;

    /// Send a signal (e.g., "SIGTERM", "SIGKILL") to a specific service process.
    #[method(name = "kill")]
    async fn kill(&self, name: String, signal: String) -> RpcResult<()>;

    /// Create a new service configuration file (e.g., "service_name.yaml")
    /// with the provided content (JSON map representing YAML structure).
    /// Returns a success message string.
    #[method(name = "create")]
    async fn create(&self, name: String, content: Map<String, Value>) -> RpcResult<String>;

    /// Delete a service configuration file.
    /// Returns a success message string.
    #[method(name = "delete")]
    async fn delete(&self, name: String) -> RpcResult<String>;

    /// Get the content of a service configuration file as a JSON Value.
    #[method(name = "get")]
    async fn get(&self, name: String) -> RpcResult<Value>;
}

// TODO: write an wrapper function that encapsulate internal zinit errors to an ErrorObjectOwned (to which we can pass our own error message)
#[async_trait]
impl ZinitServiceApiServer for Api {
    async fn list(&self) -> RpcResult<HashMap<String, String>> {
        let services = self
            .zinit
            .list()
            .await
            .map_err(|_| ErrorObjectOwned::from(ErrorCode::InternalError))?;

        let mut map: HashMap<String, String> = HashMap::new();
        for service in services {
            let state = self
                .zinit
                .status(&service)
                .await
                .map_err(|e| ErrorObjectOwned::from(ErrorCode::InternalError))?;
            map.insert(service, format!("{:?}", state.state));
        }
        Ok(map)
    }

    async fn status(&self, name: String) -> RpcResult<Status> {
        let status = self
            .zinit
            .status(&name)
            .await
            .map_err(|_| ErrorObjectOwned::from(ErrorCode::InternalError))?;

        let result = Status {
            name: name.clone(),
            pid: status.pid.as_raw() as u32,
            state: format!("{:?}", status.state),
            target: format!("{:?}", status.target),
            after: {
                let mut after = HashMap::new();
                for service in status.service.after {
                    let status = match self.zinit.status(&service).await {
                        Ok(dep) => dep.state,
                        Err(_) => crate::zinit::State::Unknown,
                    };
                    after.insert(service, format!("{:?}", status));
                }
                after
            },
        };

        Ok(result)
    }

    async fn start(&self, name: String) -> RpcResult<()> {
        self.zinit
            .start(name)
            .await
            .map_err(|_| ErrorObjectOwned::from(ErrorCode::ServerError(SERVICE_IS_UP)))
    }

    async fn stop(&self, name: String) -> RpcResult<()> {
        self.zinit
            .stop(name)
            .await
            .map_err(|_| ErrorObjectOwned::from(ErrorCode::InternalError))
    }

    async fn monitor(&self, name: String) -> RpcResult<()> {
        if let Ok((name_str, service)) = config::load(format!("{}.yaml", name))
            .map_err(|_| ErrorObjectOwned::from(ErrorCode::InternalError))
        {
            self.zinit
                .monitor(name_str, service)
                .await
                .map_err(|_| ErrorObjectOwned::from(ErrorCode::InternalError))
        } else {
            Err(ErrorObjectOwned::from(ErrorCode::InternalError))
        }
    }

    async fn forget(&self, name: String) -> RpcResult<()> {
        self.zinit
            .forget(name)
            .await
            .map_err(|_| ErrorObjectOwned::from(ErrorCode::InternalError))
    }

    async fn kill(&self, name: String, signal: String) -> RpcResult<()> {
        if let Ok(sig) = nix::sys::signal::Signal::from_str(&signal.to_uppercase()) {

        self.zinit
            .kill(name, sig)
            .await
            .map_err(|_e| ErrorObjectOwned::from(ErrorCode::InternalError))
        } else {
            Err(ErrorObjectOwned::from(ErrorCode::InternalError))
        }
    }

    async fn create(&self, name: String, content: Map<String, Value>) -> RpcResult<String> {
        use std::fs;
        use std::io::Write;
        use std::path::PathBuf;

        // Validate service name (no path traversal, valid characters)
        if name.contains('/') || name.contains('\\') || name.contains('.') {
            return Err(ErrorObjectOwned::from(ErrorCode::InternalError));
        }

        // Construct the file path
        let file_path = PathBuf::from(format!("{}.yaml", name));

        // Check if the service file already exists
        if file_path.exists() {
            return Err(ErrorObjectOwned::from(ErrorCode::ServerError(SERVICE_ALREADY_EXISTS)));
        }

        // Convert the JSON content to YAML
        let yaml_content = serde_yaml::to_string(&content)
            .map_err(|_| ErrorObjectOwned::from(ErrorCode::InternalError))?;

        // Write the YAML content to the file
        let mut file = fs::File::create(&file_path)
            .map_err(|_| ErrorObjectOwned::from(ErrorCode::ServerError(SERVICE_FILE_ERROR)))?;

        file.write_all(yaml_content.as_bytes())
            .map_err(|_| ErrorObjectOwned::from(ErrorCode::ServerError(SERVICE_FILE_ERROR)))?;

        Ok(format!("Service '{}' created successfully", name))
    }

    async fn delete(&self, name: String) -> RpcResult<String> {
        use std::fs;
        use std::path::PathBuf;

        // Validate service name (no path traversal, valid characters)
        if name.contains('/') || name.contains('\\') || name.contains('.') {
            return Err(ErrorObjectOwned::from(ErrorCode::InternalError));
        }

        // Construct the file path
        let file_path = PathBuf::from(format!("{}.yaml", name));

        // Check if the service file exists
        if !file_path.exists() {
            return Err(ErrorObjectOwned::from(ErrorCode::ServerError(SERVICE_NOT_FOUND)));
        }

        // Delete the file
        fs::remove_file(&file_path)
            .map_err(|_| ErrorObjectOwned::from(ErrorCode::ServerError(SERVICE_FILE_ERROR)))?;

        Ok(format!("Service '{}' deleted successfully", name))
    }

    async fn get(&self, name: String) -> RpcResult<Value> {
        use std::fs;
        use std::path::PathBuf;

        // Validate service name (no path traversal, valid characters)
        if name.contains('/') || name.contains('\\') || name.contains('.') {
            return Err(ErrorObjectOwned::from(ErrorCode::InternalError));
        }

        // Construct the file path
        let file_path = PathBuf::from(format!("{}.yaml", name));

        // Check if the service file exists
        if !file_path.exists() {
            return Err(ErrorObjectOwned::from(ErrorCode::ServerError(SERVICE_NOT_FOUND)));
        }

        // Read the file content
        let yaml_content = fs::read_to_string(&file_path)
            .map_err(|_| ErrorObjectOwned::from(ErrorCode::ServerError(SERVICE_FILE_ERROR)))?;

        // Parse YAML to JSON
        let yaml_value: serde_yaml::Value = serde_yaml::from_str(&yaml_content)
            .map_err(|_| ErrorObjectOwned::from(ErrorCode::InternalError))?;

        // Convert YAML value to JSON value
        let json_value = serde_json::to_value(yaml_value)
            .map_err(|_| ErrorObjectOwned::from(ErrorCode::InternalError))?;

        Ok(json_value)
    }
}

/// RPC methods for system-level operations.
#[rpc(server, client, namespace = "system")]
pub trait ZinitSystemApi {
    /// Initiate system shutdown process.
    #[method(name = "shutdown")]
    async fn shutdown(&self) -> RpcResult<()>;

    /// Initiate system reboot process.
    #[method(name = "reboot")]
    async fn reboot(&self) -> RpcResult<()>;
}

#[async_trait]
impl ZinitSystemApiServer for Api {
    async fn shutdown(&self) -> RpcResult<()> {
        self.zinit.shutdown().await.map_err(|_e| ErrorObjectOwned::from(ErrorCode::ServerError(SHUTTING_DOWN)))
    }
    
    async fn reboot(&self) -> RpcResult<()> {
        self.zinit.reboot().await.map_err(|_| ErrorObjectOwned::from(ErrorCode::InternalError))
    }
}

/// RPC subscription methods for streaming data.
#[rpc(server, client, namespace = "stream")]
pub trait ZinitLoggingApi {
    #[method(name = "currentLogs")]
    async fn logs(&self, name: Option<String>) -> RpcResult<Vec<String>>;
    /// Subscribe to log messages generated by zinit and monitored services.
    /// An optional filter can be provided to only receive logs containing the filter string.
    /// The subscription returns a stream of log lines (String).
    #[subscription(name = "subscribeLogs", item = String, unsubscribe = "log_unsubscribe")]
    async fn log_subscribe(&self, filter: Option<String>);
}

#[async_trait]
impl ZinitLoggingApiServer for Api {
    async fn logs(&self, name: Option<String>) -> RpcResult<Vec<String>> {
        self.zinit.logs()
    }
}
