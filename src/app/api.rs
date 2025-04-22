use crate::zinit::ZInit;
use anyhow::{bail, Context, Result};
use jsonrpsee::server::ServerHandle;
use reth_ipc::server::Builder;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use super::rpc::{ZinitLoggingApiServer, ZinitRpcApiServer, ZinitServiceApiServer, ZinitSystemApiServer};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
struct ZinitResponse {
    pub state: ZinitState,
    pub body: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum ZinitState {
    Ok,
    Error,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub struct Status {
    pub name: String,
    pub pid: u32,
    pub state: String,
    pub target: String,
    pub after: HashMap<String, String>,
}

pub struct ApiServer {
    _handle: ServerHandle,
}

#[derive(Clone)]
pub struct Api {
    pub zinit: ZInit,
    pub http_server_handle: Arc<Mutex<Option<jsonrpsee::server::ServerHandle>>>,
}

impl Api {
    pub fn new(zinit: ZInit) -> Api {
        Api {
            zinit,
            http_server_handle: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn serve(&self, endpoint: String) -> Result<ApiServer> {

        let server = Builder::default().build(endpoint);
        let mut module = ZinitRpcApiServer::into_rpc(self.clone());
        module.merge(ZinitSystemApiServer::into_rpc(self.clone()))?;
        module.merge(ZinitServiceApiServer::into_rpc(self.clone()))?;
        module.merge(ZinitLoggingApiServer::into_rpc(self.clone()))?;


        let _handle = server.start(module).await?;

        Ok(ApiServer { _handle })
    }
    
    /// Start an HTTP/RPC server at a specified address
    pub async fn start_http_server(&self, address: String) -> Result<String> {
        // Parse the address string
        let socket_addr = address.parse::<std::net::SocketAddr>()
            .context("Failed to parse socket address")?;
            
        // Create the JSON-RPC server
        let server_rpc = jsonrpsee::server::ServerBuilder::default()
            .build(socket_addr)
            .await?;
            
        // Create and merge all API modules
        let mut rpc_module = ZinitRpcApiServer::into_rpc(self.clone());
        rpc_module.merge(ZinitSystemApiServer::into_rpc(self.clone()))?;
        rpc_module.merge(ZinitServiceApiServer::into_rpc(self.clone()))?;
        rpc_module.merge(ZinitLoggingApiServer::into_rpc(self.clone()))?;

        // Start the server
        let handle = server_rpc.start(rpc_module);
        
        // Store the handle
        let mut http_handle = self.http_server_handle.lock().await;
        *http_handle = Some(handle);
        
        Ok(format!("HTTP/RPC server started at {}", address))
    }
    
    /// Stop the HTTP/RPC server if running
    pub async fn stop_http_server(&self) -> Result<()> {
        let mut http_handle = self.http_server_handle.lock().await;
        
        if http_handle.is_some() {
            // The handle is automatically dropped, which should stop the server
            *http_handle = None;
            Ok(())
        } else {
            bail!("No HTTP/RPC server is currently running")
        }
    }
}