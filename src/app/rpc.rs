use crate::app::api::{ChildStats, Stats, Status};
use crate::zinit::config;
use async_trait::async_trait;
use jsonrpsee::core::{RpcResult, SubscriptionResult};
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::types::{ErrorCode, ErrorObjectOwned};
use jsonrpsee::PendingSubscriptionSink;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::str::FromStr;
use tokio_stream::StreamExt;

use crate::zinit::errors::ZInitError;
use anyhow::Error as AnyError;

use super::api::Api;

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

fn code_name_from(code: i32) -> &'static str {
    match code {
        SERVICE_NOT_FOUND => "ServiceNotFound",
        SERVICE_ALREADY_MONITORED => "ServiceAlreadyMonitored",
        SERVICE_IS_UP => "ServiceIsUp",
        SERVICE_IS_DOWN => "ServiceIsDown",
        INVALID_SIGNAL => "InvalidSignal",
        CONFIG_ERROR => "ConfigError",
        SHUTTING_DOWN => "ShuttingDown",
        SERVICE_ALREADY_EXISTS => "ServiceAlreadyExists",
        SERVICE_FILE_ERROR => "ServiceFileError",
        _ => "InternalError",
    }
}

// Map ZInit/anyhow error -> JSON-RPC ErrorObjectOwned with structured details.
fn zinit_err_to_rpc(
    err: AnyError,
    action: &str,
    service: Option<&str>,
    default_code: i32,
) -> ErrorObjectOwned {
    // Choose code/name/message from domain errors when possible
    let (code, code_name, top_msg) = match err.downcast_ref::<ZInitError>() {
        Some(ZInitError::UnknownService { name }) => (
            SERVICE_NOT_FOUND,
            "ServiceNotFound",
            format!("service '{}' not found", name),
        ),
        Some(ZInitError::ServiceAlreadyMonitored { name }) => (
            SERVICE_ALREADY_MONITORED,
            "ServiceAlreadyMonitored",
            format!("service '{}' already monitored", name),
        ),
        Some(ZInitError::ServiceIsUp { name }) => (
            SERVICE_IS_UP,
            "ServiceIsUp",
            format!("service '{}' is up", name),
        ),
        Some(ZInitError::ServiceIsDown { name }) => (
            SERVICE_IS_DOWN,
            "ServiceIsDown",
            format!("service '{}' is down", name),
        ),
        Some(ZInitError::ShuttingDown) => (
            SHUTTING_DOWN,
            "ShuttingDown",
            "system is shutting down".to_string(),
        ),
        Some(ZInitError::InvalidStateTransition { message }) => {
            (-32009, "InvalidStateTransition", message.clone())
        }
        Some(ZInitError::DependencyError { message }) => {
            (-32010, "DependencyError", message.clone())
        }
        Some(ZInitError::ProcessError { message }) => (-32011, "ProcessError", message.clone()),
        None => (default_code, code_name_from(default_code), err.to_string()),
    };

    // Build cause_chain from std::error::Error sources.
    let mut cause_chain: Vec<String> = Vec::new();
    let mut src = err.source();
    while let Some(s) = src {
        cause_chain.push(s.to_string());
        src = s.source();
    }

    let retryable = matches!(code, SERVICE_ALREADY_MONITORED | SERVICE_IS_DOWN);

    let data = json!({
        "code_name": code_name,
        "service": service,
        "action": action,
        "retryable": retryable,
        "cause_chain": if cause_chain.is_empty() { serde_json::Value::Null } else { serde_json::Value::Array(cause_chain.into_iter().map(serde_json::Value::String).collect()) },
        "hint": match code {
            SERVICE_NOT_FOUND => "Ensure the service is monitored or the name is correct",
            SERVICE_ALREADY_MONITORED => "The service is already monitored; call service_forget or control it with start/stop",
            SERVICE_IS_UP => "Service is already up; use service_stop or restart",
            SERVICE_IS_DOWN => "Service is down; start it before sending signals or requesting stats",
            SHUTTING_DOWN => "Wait for shutdown to complete or avoid mutating operations during shutdown",
            SERVICE_FILE_ERROR => "Check filesystem permissions, disk space, and path",
            _ => "Enable verbose logs and inspect cause_chain for details"
        }
    });

    let message = format!("{}: {}", action, top_msg);
    ErrorObjectOwned::owned(code, message, Some(data))
}

fn invalid_service_name_error(action: &str, name: &str) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(
        ErrorCode::InvalidParams.code(),
        format!("{action}: invalid service name '{}'", name),
        Some(json!({
            "code_name": "InvalidServiceName",
            "action": action,
            "service": name,
            "hint": "Name must not contain '/', '\\\\' or '.'"
        })),
    )
}

fn invalid_signal_error(signal: &str) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(
        INVALID_SIGNAL,
        format!("parsing signal: invalid signal '{}'", signal),
        Some(json!({
            "code_name": "InvalidSignal",
            "action": "parsing signal",
            "signal": signal,
            "hint": "Use a valid POSIX signal like SIGTERM or SIGKILL"
        })),
    )
}

fn service_file_error(action: &str, name: &str, detail: &str) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(
        SERVICE_FILE_ERROR,
        format!("{action}: {detail}"),
        Some(json!({
            "code_name": "ServiceFileError",
            "action": action,
            "service": name,
            "hint": "Check permissions and filesystem availability"
        })),
    )
}

/// RPC methods for discovery.
#[rpc(server, client)]
pub trait ZinitRpcApi {
    /// Returns the OpenRPC specification as a string.
    #[method(name = "rpc.discover")]
    async fn discover(&self) -> RpcResult<String>;
}

#[async_trait]
impl ZinitRpcApiServer for Api {
    async fn discover(&self) -> RpcResult<String> {
        Ok(OPENRPC_SPEC.to_string())
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

    /// Get memory and CPU usage statistics for a service.
    #[method(name = "stats")]
    async fn stats(&self, name: String) -> RpcResult<Stats>;
}

#[async_trait]
impl ZinitServiceApiServer for Api {
    async fn list(&self) -> RpcResult<HashMap<String, String>> {
        let services = self.zinit.list().await.map_err(|e| {
            zinit_err_to_rpc(e, "listing services", None, ErrorCode::InternalError.code())
        })?;

        let mut map: HashMap<String, String> = HashMap::new();
        for service in services {
            let state = self.zinit.status(&service).await.map_err(|e| {
                zinit_err_to_rpc(
                    e,
                    "getting status",
                    Some(&service),
                    ErrorCode::InternalError.code(),
                )
            })?;
            map.insert(service, format!("{:?}", state.state));
        }
        Ok(map)
    }

    async fn status(&self, name: String) -> RpcResult<Status> {
        let status = self.zinit.status(&name).await.map_err(|e| {
            zinit_err_to_rpc(
                e,
                "getting status",
                Some(&name),
                ErrorCode::InternalError.code(),
            )
        })?;

        let result = Status {
            name: name.clone(),
            pid: status.pid.as_raw() as u32,
            state: format!("{:?}", status.state),
            target: format!("{:?}", status.target),
            after: {
                let mut after = HashMap::new();
                for service in status.service.after {
                    let dep_state = match self.zinit.status(&service).await {
                        Ok(dep) => dep.state,
                        Err(_) => crate::zinit::State::Unknown,
                    };
                    after.insert(service, format!("{:?}", dep_state));
                }
                after
            },
        };

        Ok(result)
    }

    async fn start(&self, name: String) -> RpcResult<()> {
        self.zinit.start(name.clone()).await.map_err(|e| {
            zinit_err_to_rpc(
                e,
                "starting service",
                Some(&name),
                ErrorCode::InternalError.code(),
            )
        })
    }

    async fn stop(&self, name: String) -> RpcResult<()> {
        self.zinit.stop(name.clone()).await.map_err(|e| {
            zinit_err_to_rpc(
                e,
                "stopping service",
                Some(&name),
                ErrorCode::InternalError.code(),
            )
        })
    }

    async fn monitor(&self, name: String) -> RpcResult<()> {
        // Validate service name
        if name.contains('/') || name.contains('\\') || name.contains('.') {
            return Err(invalid_service_name_error("monitoring service", &name));
        }

        let (name_str, service) = config::load(format!("{}.yaml", name)).map_err(|e| {
            ErrorObjectOwned::owned(
                CONFIG_ERROR,
                format!("monitoring service: failed to load '{}.yaml'", name),
                Some(json!({
                    "code_name": "ConfigError",
                    "action": "loading service config",
                    "service": name,
                    "hint": "Verify the YAML file exists and is valid"
                })),
            )
        })?;

        self.zinit
            .monitor(name_str.clone(), service)
            .await
            .map_err(|e| {
                zinit_err_to_rpc(
                    e,
                    "monitoring service",
                    Some(&name_str),
                    ErrorCode::InternalError.code(),
                )
            })
    }

    async fn forget(&self, name: String) -> RpcResult<()> {
        self.zinit.forget(name.clone()).await.map_err(|e| {
            zinit_err_to_rpc(
                e,
                "forgetting service",
                Some(&name),
                ErrorCode::InternalError.code(),
            )
        })
    }

    async fn kill(&self, name: String, signal: String) -> RpcResult<()> {
        if let Ok(sig) = nix::sys::signal::Signal::from_str(&signal.to_uppercase()) {
            self.zinit.kill(name.clone(), sig).await.map_err(|e| {
                zinit_err_to_rpc(
                    e,
                    "sending signal",
                    Some(&name),
                    ErrorCode::InternalError.code(),
                )
            })
        } else {
            Err(invalid_signal_error(&signal))
        }
    }

    async fn create(&self, name: String, content: Map<String, Value>) -> RpcResult<String> {
        use std::fs;
        use std::io::Write;
        use std::path::PathBuf;

        // Validate service name (no path traversal, valid characters)
        if name.contains('/') || name.contains('\\') || name.contains('.') {
            return Err(invalid_service_name_error("creating service", &name));
        }

        // Construct the file path
        let file_path = PathBuf::from(format!("{}.yaml", &name));

        // Check if the service file already exists
        if file_path.exists() {
            return Err(ErrorObjectOwned::owned(
                SERVICE_ALREADY_EXISTS,
                format!("creating service: Service '{}' already exists", name),
                Some(json!({
                    "code_name": "ServiceAlreadyExists",
                    "action": "creating service file",
                    "service": name,
                    "hint": "Use a different name or delete the existing service first"
                })),
            ));
        }

        // Convert the JSON content to YAML
        let yaml_content = serde_yaml::to_string(&content).map_err(|e| {
            ErrorObjectOwned::owned(
                CONFIG_ERROR,
                "creating service: failed to convert content to YAML".to_string(),
                Some(json!({
                    "code_name": "ConfigError",
                    "action": "serializing service config",
                    "service": name,
                    "hint": "Ensure content is valid according to schema"
                })),
            )
        })?;

        // Write the YAML content to the file
        let mut file = fs::File::create(&file_path).map_err(|_| {
            service_file_error(
                "creating service file",
                &name,
                "failed to create service file",
            )
        })?;

        file.write_all(yaml_content.as_bytes()).map_err(|_| {
            service_file_error(
                "writing service file",
                &name,
                "failed to write service file",
            )
        })?;

        Ok(format!("Service '{}' created successfully", name))
    }

    async fn delete(&self, name: String) -> RpcResult<String> {
        use std::fs;
        use std::path::PathBuf;

        // Validate service name (no path traversal, valid characters)
        if name.contains('/') || name.contains('\\') || name.contains('.') {
            return Err(invalid_service_name_error("deleting service", &name));
        }

        // Construct the file path
        let file_path = PathBuf::from(format!("{}.yaml", &name));

        // Check if the service file exists
        if !file_path.exists() {
            return Err(ErrorObjectOwned::owned(
                SERVICE_NOT_FOUND,
                format!("deleting service: Service '{}' not found", name),
                Some(json!({
                    "code_name": "ServiceNotFound",
                    "action": "deleting service file",
                    "service": name,
                    "hint": "Ensure the service file exists"
                })),
            ));
        }

        // Delete the file
        fs::remove_file(&file_path).map_err(|_| {
            service_file_error(
                "deleting service file",
                &name,
                "failed to delete service file",
            )
        })?;

        Ok(format!("Service '{}' deleted successfully", name))
    }

    async fn get(&self, name: String) -> RpcResult<Value> {
        use std::fs;
        use std::path::PathBuf;

        // Validate service name (no path traversal, valid characters)
        if name.contains('/') || name.contains('\\') || name.contains('.') {
            return Err(invalid_service_name_error("getting service", &name));
        }

        // Construct the file path
        let file_path = PathBuf::from(format!("{}.yaml", &name));

        // Check if the service file exists
        if !file_path.exists() {
            return Err(ErrorObjectOwned::owned(
                SERVICE_NOT_FOUND,
                format!("getting service: Service '{}' not found", name),
                Some(json!({
                    "code_name": "ServiceNotFound",
                    "action": "reading service file",
                    "service": name,
                    "hint": "Ensure the service file exists"
                })),
            ));
        }

        // Read the file content
        let yaml_content = fs::read_to_string(&file_path).map_err(|_| {
            service_file_error("reading service file", &name, "failed to read service file")
        })?;

        // Parse YAML to JSON
        let yaml_value: serde_yaml::Value = serde_yaml::from_str(&yaml_content).map_err(|_| {
            ErrorObjectOwned::owned(
                CONFIG_ERROR,
                "getting service: failed to parse YAML".to_string(),
                Some(json!({
                    "code_name": "ConfigError",
                    "action": "parsing service file",
                    "service": name,
                    "hint": "Ensure the YAML is valid"
                })),
            )
        })?;

        // Convert YAML value to JSON value
        let json_value = serde_json::to_value(yaml_value).map_err(|_| {
            ErrorObjectOwned::owned(
                CONFIG_ERROR,
                "getting service: failed to convert YAML to JSON".to_string(),
                Some(json!({
                    "code_name": "ConfigError",
                    "action": "converting YAML to JSON",
                    "service": name
                })),
            )
        })?;

        Ok(json_value)
    }

    async fn stats(&self, name: String) -> RpcResult<Stats> {
        let stats = self.zinit.stats(&name).await.map_err(|e| {
            zinit_err_to_rpc(
                e,
                "collecting stats",
                Some(&name),
                ErrorCode::InternalError.code(),
            )
        })?;

        let result = Stats {
            name: name.clone(),
            pid: stats.pid as u32,
            memory_usage: stats.memory_usage,
            cpu_usage: stats.cpu_usage,
            children: stats
                .children
                .into_iter()
                .map(|child| ChildStats {
                    pid: child.pid as u32,
                    memory_usage: child.memory_usage,
                    cpu_usage: child.cpu_usage,
                })
                .collect(),
        };

        Ok(result)
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

    /// Start an HTTP/RPC server at the specified address
    #[method(name = "start_http_server")]
    async fn start_http_server(&self, address: String) -> RpcResult<String>;

    /// Stop the HTTP/RPC server if running
    #[method(name = "stop_http_server")]
    async fn stop_http_server(&self) -> RpcResult<()>;
}

#[async_trait]
impl ZinitSystemApiServer for Api {
    async fn shutdown(&self) -> RpcResult<()> {
        self.zinit
            .shutdown()
            .await
            .map_err(|e| zinit_err_to_rpc(e, "system shutdown", None, SHUTTING_DOWN))
    }

    async fn reboot(&self) -> RpcResult<()> {
        self.zinit.reboot().await.map_err(|e| {
            zinit_err_to_rpc(e, "system reboot", None, ErrorCode::InternalError.code())
        })
    }

    async fn start_http_server(&self, address: String) -> RpcResult<String> {
        // Call the method from the API implementation
        match crate::app::api::Api::start_http_server(self, address).await {
            Ok(result) => Ok(result),
            Err(e) => Err(zinit_err_to_rpc(
                AnyError::from(e),
                "starting http server",
                None,
                ErrorCode::InternalError.code(),
            )),
        }
    }

    async fn stop_http_server(&self) -> RpcResult<()> {
        // Call the method from the API implementation
        match crate::app::api::Api::stop_http_server(self).await {
            Ok(_) => Ok(()),
            Err(e) => Err(zinit_err_to_rpc(
                AnyError::from(e),
                "stopping http server",
                None,
                ErrorCode::InternalError.code(),
            )),
        }
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
    #[subscription(name = "subscribeLogs", item = String)]
    async fn log_subscribe(&self, filter: Option<String>) -> SubscriptionResult;
}

#[async_trait]
impl ZinitLoggingApiServer for Api {
    async fn logs(&self, name: Option<String>) -> RpcResult<Vec<String>> {
        let filter = name.map(|n| format!("{n}:"));
        Ok(
            tokio_stream::wrappers::ReceiverStream::new(self.zinit.logs(true, false).await)
                .filter_map(|l| {
                    if let Some(ref filter) = filter {
                        if l[4..].starts_with(filter) {
                            Some(l.to_string())
                        } else {
                            None
                        }
                    } else {
                        Some(l.to_string())
                    }
                })
                .collect()
                .await,
        )
    }

    async fn log_subscribe(
        &self,
        sink: PendingSubscriptionSink,
        name: Option<String>,
    ) -> SubscriptionResult {
        let sink = sink.accept().await?;
        let filter = name.map(|n| format!("{n}:"));
        let mut stream =
            tokio_stream::wrappers::ReceiverStream::new(self.zinit.logs(false, true).await)
                .filter_map(|l| {
                    if let Some(ref filter) = filter {
                        if l[4..].starts_with(filter) {
                            Some(l.to_string())
                        } else {
                            None
                        }
                    } else {
                        Some(l.to_string())
                    }
                });
        while let Some(log) = stream.next().await {
            if sink
                .send(serde_json::value::to_raw_value(&log)?)
                .await
                .is_err()
            {
                break;
            }
        }

        Ok(())
    }
}
