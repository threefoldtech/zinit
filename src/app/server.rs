use crate::zinit::{config, ZInit, State as ZinitInternalState};
use crate::app::api_trait::{
    ZinitRpcApiServer, ZinitServiceApiServer, ZinitSystemApiServer, /* ZinitLoggingApiServer, */ RpcResult, // Keep logging trait commented out
};
// use crate::app::api_trait::ZinitLoggingApiServer; // Keep commented out
use crate::app::types::Status; // Use Status from the types module
use anyhow::{Context, Result};
use jsonrpsee::{
    server::{ServerBuilder, ServerHandle, RpcModule },
    types::{ErrorObjectOwned, Params},
};
use log::{info, error, warn}; // Use log crate macros
use nix::sys::signal;
use serde_json::{self, Value, Map};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;

// Include the OpenRPC specification - still needed for the discover method
const OPENRPC_SPEC: &str = include_str!("../../openrpc.json");

// Custom error codes for Zinit (can be mapped to ErrorObjectOwned)
const SERVICE_NOT_FOUND: i32 = -32000;
const SERVICE_ALREADY_MONITORED: i32 = -32001;
const SERVICE_IS_UP: i32 = -32002;
const SERVICE_IS_DOWN: i32 = -32003;
const INVALID_SIGNAL: i32 = -32004;
const CONFIG_ERROR: i32 = -32005;
const SHUTTING_DOWN: i32 = -32006;
const SERVICE_ALREADY_EXISTS: i32 = -32007;
const SERVICE_FILE_ERROR: i32 = -32008;

// Helper to convert anyhow::Error to jsonrpsee::types::ErrorObjectOwned
fn to_rpc_error(err: anyhow::Error) -> ErrorObjectOwned {
    // Attempt to map specific Zinit errors to codes
    let code = if err.to_string().contains("service name") && err.to_string().contains("unknown") {
        SERVICE_NOT_FOUND
    } else if err.to_string().contains("already monitored") {
        SERVICE_ALREADY_MONITORED
    } else if err.to_string().contains("service") && err.to_string().contains("is up") {
        SERVICE_IS_UP
    } else if err.to_string().contains("service") && err.to_string().contains("is down") {
        SERVICE_IS_DOWN
    } else if err.to_string().contains("signal") {
        INVALID_SIGNAL
    } else if err.to_string().contains("config") {
        CONFIG_ERROR
    } else if err.to_string().contains("shutting down") {
        SHUTTING_DOWN
    } else if err.to_string().contains("Service") && err.to_string().contains("already exists") {
        SERVICE_ALREADY_EXISTS
    } else if err.to_string().contains("Failed to") && (err.to_string().contains("service file") || err.to_string().contains("configuration")) {
        SERVICE_FILE_ERROR
    } else {
        jsonrpsee::types::error::INTERNAL_ERROR_CODE // Default to internal error
    };

    ErrorObjectOwned::owned(code, err.to_string(), None::<()>)
}


#[derive(Clone)] // Api needs to be Clone to be used with jsonrpsee server
pub struct Api {
    zinit: ZInit,
    config_dir: PathBuf, // Keep config dir if needed for service file operations
}

impl Api {
    // Simplified constructor
    pub fn new(zinit: ZInit, config_dir: PathBuf) -> Self {
        Api { zinit, config_dir }
    }

    // Start the jsonrpsee server (HTTP & WS)
    pub async fn serve(
        self,
        http_addr: SocketAddr,
        ws_addr: SocketAddr,
    ) -> Result<(ServerHandle, SocketAddr, SocketAddr)> {
        let server = ServerBuilder::default()
            .http_only() // Start with HTTP only builder
            .build(http_addr)
            .await
            .context("Failed to build HTTP server")?;

        let ws_server = ServerBuilder::default()
            .ws_only() // Start with WS only builder
            .build(ws_addr)
            .await
            .context("Failed to build WebSocket server")?;

        let http_bound_addr = server.local_addr()?;
        let ws_bound_addr = ws_server.local_addr()?;

        let mut module = RpcModule::new(self.clone()); // Use self directly as context

        // Merge methods from all API traits into the module
        module.merge(ZinitRpcApiServer::into_rpc(self.clone()))?;
        module.merge(ZinitServiceApiServer::into_rpc(self.clone()))?;
        module.merge(ZinitSystemApiServer::into_rpc(self.clone()))?;
        // module.merge(ZinitLoggingApiServer::into_rpc(self.clone()))?; // Keep commented out

        // server.start() returns ServerHandle directly
        let handle = server.start(module.clone());
        let ws_handle = ws_server.start(module); // Note: This starts a *separate* server instance with the same methods

        info!("HTTP JSON-RPC server listening on {}", http_bound_addr);
        info!("WebSocket JSON-RPC server listening on {}", ws_bound_addr);

        // We might want to return both handles or manage them together
        // For now, returning the first handle and both addresses
        Ok((handle, http_bound_addr, ws_bound_addr))
    }

    // --- Helper functions adapted from old implementation ---
    // These are now private helpers called by the trait implementations below

    async fn list_internal(&self) -> Result<HashMap<String, String>> {
        let services = self.zinit.list().await?;
        let mut map = HashMap::new();
        for service in services {
            let state = self.zinit.status(&service).await?;
            map.insert(service, format!("{:?}", state.state));
        }
        Ok(map)
    }

    async fn monitor_internal<S: AsRef<str>>(&self, name: S) -> Result<()> {
        let name_str = name.as_ref();
        let config_path = self.config_dir.join(format!("{}.yaml", name_str));
        let (name_loaded, service) = config::load(&config_path)
            .with_context(|| format!("Failed to load service config: {}", config_path.display()))?;
        if name_loaded != name_str {
             warn!("Loaded service name '{}' does not match requested name '{}'", name_loaded, name_str);
        }
        self.zinit.monitor(name_str.to_string(), service).await?;
        Ok(())
    }

    async fn forget_internal<S: AsRef<str>>(&self, name: S) -> Result<()> {
        self.zinit.forget(name).await?;
        Ok(())
    }

    async fn stop_internal<S: AsRef<str>>(&self, name: S) -> Result<()> {
        self.zinit.stop(name).await?;
        Ok(())
    }

    async fn shutdown_internal(&self) -> Result<()> {
        let zinit = self.zinit.clone();
        tokio::spawn(async move {
            if let Err(err) = zinit.shutdown().await {
                error!("Failed to execute shutdown: {}", err);
            }
        });
        Ok(())
    }

    async fn reboot_internal(&self) -> Result<()> {
        let zinit = self.zinit.clone();
        tokio::spawn(async move {
            if let Err(err) = zinit.reboot().await {
                error!("Failed to execute reboot: {}", err);
            }
        });
        Ok(())
    }

    async fn start_internal<S: AsRef<str>>(&self, name: S) -> Result<()> {
        self.zinit.start(name).await?;
        Ok(())
    }

    async fn kill_internal<S: AsRef<str>>(&self, name: S, sig_str: S) -> Result<()> {
        let sig = signal::Signal::from_str(&sig_str.as_ref().to_uppercase())
            .map_err(|_| anyhow::anyhow!("Invalid signal: {}", sig_str.as_ref()))?;
        self.zinit.kill(name, sig).await?;
        Ok(())
    }

    async fn status_internal<S: AsRef<str>>(&self, name: S) -> Result<Status> {
        let name_ref = name.as_ref();
        let status = self.zinit.status(name_ref).await?;

        let mut after_map = HashMap::new();
        for service_dep in status.service.after.iter() {
             let dep_status = match self.zinit.status(service_dep).await {
                 Ok(dep) => dep.state,
                 Err(_) => ZinitInternalState::Unknown,
             };
             after_map.insert(service_dep.clone(), format!("{:?}", dep_status));
        }

        let result = Status {
            name: name_ref.to_string(),
            pid: status.pid.as_raw() as u32,
            state: format!("{:?}", status.state),
            target: format!("{:?}", status.target),
            after: after_map,
        };

        Ok(result)
    }

    async fn create_service_internal<S: AsRef<str>>(
        &self,
        name: S,
        content: &Map<String, Value>,
    ) -> Result<String> {
        use tokio::fs;
        use tokio::io::AsyncWriteExt;

        let name = name.as_ref();
        if name.contains('/') || name.contains('\\') || name.contains('.') {
            anyhow::bail!("Invalid service name: must not contain '/', '\\', or '.'");
        }
        let file_path = self.config_dir.join(format!("{}.yaml", name));
        if fs::metadata(&file_path).await.is_ok() {
            anyhow::bail!("Service '{}' already exists at {}", name, file_path.display());
        }
        let yaml_content = serde_yaml::to_string(content)
            .context("Failed to convert service configuration to YAML")?;
        let mut file = fs::File::create(&file_path)
            .await
            .with_context(|| format!("Failed to create service file: {}", file_path.display()))?;
        file.write_all(yaml_content.as_bytes())
            .await
            .context("Failed to write service configuration")?;
        Ok(format!("Service '{}' created successfully at {}", name, file_path.display()))
    }

    async fn delete_service_internal<S: AsRef<str>>(&self, name: S) -> Result<String> {
        use tokio::fs;
        let name = name.as_ref();
        if name.contains('/') || name.contains('\\') || name.contains('.') {
            anyhow::bail!("Invalid service name: must not contain '/', '\\', or '.'");
        }
        let file_path = self.config_dir.join(format!("{}.yaml", name));
        if fs::metadata(&file_path).await.is_err() {
             anyhow::bail!("Service '{}' not found at {}", name, file_path.display());
        }
        fs::remove_file(&file_path)
            .await
            .with_context(|| format!("Failed to delete service file: {}", file_path.display()))?;
        Ok(format!("Service '{}' deleted successfully from {}", name, file_path.display()))
    }

    async fn get_service_internal<S: AsRef<str>>(&self, name: S) -> Result<Value> {
        use tokio::fs;
        let name = name.as_ref();
        if name.contains('/') || name.contains('\\') || name.contains('.') {
            anyhow::bail!("Invalid service name: must not contain '/', '\\', or '.'");
        }
        let file_path = self.config_dir.join(format!("{}.yaml", name));
        if fs::metadata(&file_path).await.is_err() {
             anyhow::bail!("Service '{}' not found at {}", name, file_path.display());
        }
        let yaml_content = fs::read_to_string(&file_path)
            .await
            .with_context(|| format!("Failed to read service file: {}", file_path.display()))?;
        let yaml_value: serde_yaml::Value =
            serde_yaml::from_str(&yaml_content).context("Failed to parse YAML content")?;
        let json_value =
            serde_json::to_value(yaml_value).context("Failed to convert YAML to JSON")?;
        Ok(json_value)
    }
}

// --- Trait Implementations ---

#[async_trait::async_trait]
impl ZinitRpcApiServer for Api {
    async fn discover(&self) -> RpcResult<Value> {
        serde_json::from_str(OPENRPC_SPEC).map_err(|err| {
            error!("Failed to parse OpenRPC spec: {}", err);
            ErrorObjectOwned::owned(
                jsonrpsee::types::error::INTERNAL_ERROR_CODE,
                format!("Failed to parse OpenRPC spec: {}", err),
                None::<()>,
            )
        })
    }
}

#[async_trait::async_trait]
impl ZinitServiceApiServer for Api {
    async fn list(&self) -> RpcResult<HashMap<String, String>> {
        self.list_internal().await.map_err(to_rpc_error)
    }

    async fn status(&self, name: String) -> RpcResult<Status> {
        self.status_internal(&name).await.map_err(to_rpc_error)
    }

    async fn start(&self, name: String) -> RpcResult<()> {
        self.start_internal(&name).await.map_err(to_rpc_error)
    }

    async fn stop(&self, name: String) -> RpcResult<()> {
        self.stop_internal(&name).await.map_err(to_rpc_error)
    }

    async fn monitor(&self, name: String) -> RpcResult<()> {
        self.monitor_internal(&name).await.map_err(to_rpc_error)
    }

    async fn forget(&self, name: String) -> RpcResult<()> {
        self.forget_internal(&name).await.map_err(to_rpc_error)
    }

    async fn kill(&self, name: String, signal: String) -> RpcResult<()> {
        self.kill_internal(&name, &signal).await.map_err(to_rpc_error)
    }

    async fn create(&self, name: String, content: Map<String, Value>) -> RpcResult<String> {
        self.create_service_internal(&name, &content).await.map_err(to_rpc_error)
    }

    async fn delete(&self, name: String) -> RpcResult<String> {
        self.delete_service_internal(&name).await.map_err(to_rpc_error)
    }

    async fn get(&self, name: String) -> RpcResult<Value> {
        self.get_service_internal(&name).await.map_err(to_rpc_error)
    }
}

#[async_trait::async_trait]
impl ZinitSystemApiServer for Api {
    async fn shutdown(&self) -> RpcResult<()> {
        self.shutdown_internal().await.map_err(to_rpc_error)
    }

    async fn reboot(&self) -> RpcResult<()> {
        self.reboot_internal().await.map_err(to_rpc_error)
    }
}

/* Keep ZinitLoggingApiServer impl block commented out
#[async_trait::async_trait]
impl ZinitLoggingApiServer for Api {
    async fn log_subscribe(
        &self,
        mut sink: SubscriptionSink,
        params: Params<'_>,
    // Use jsonrpsee::Error directly
    ) -> Result<(), RpcError> {
        sink.accept().await.map_err(|e| {
            error!("Failed to accept subscription: {}", e);
            e // Assuming accept error is jsonrpsee::Error
        })?;

        let filter: Option<String> = params.optional_params().map_err(|e_obj| {
            error!("Failed to parse subscription params: {}", e_obj);
            // Convert ErrorObjectOwned into jsonrpsee::Error
            RpcError::Call(e_obj.into())
        })?;

        info!("New log subscription accepted. Filter: {:?}", filter);

        let mut log_receiver = self.zinit.logs(true).await;
        let zinit_clone = self.zinit.clone();

        loop {
            tokio::select! {
                _ = zinit_clone.wait() => {
                    info!("Zinit shutting down, closing log subscription.");
                    break;
                },
                maybe_line = log_receiver.recv() => {
                    match maybe_line {
                        Some(line) => {
                            let line_str = line.to_string();
                            let should_send = filter.as_ref().map_or(true, |f| line_str.contains(f));

                            if should_send {
                                if let Err(e) = sink.send(&line_str).await {
                                    warn!("Failed to send log line: {}. Closing subscription.", e);
                                    break;
                                }
                            }
                        }
                        None => {
                            info!("Log stream ended. Closing subscription.");
                            break;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    // Need to implement the unsubscribe method required by the trait definition
    async fn log_unsubscribe(&self, _subscription_id: String) -> RpcResult<bool> {
         // Usually, jsonrpsee handles the closing automatically when the sink is dropped.
         // Return Ok(true) to indicate success.
         Ok(true)
     }
}
*/ // End of commented out block
