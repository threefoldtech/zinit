use crate::zinit::ZInit;
use anyhow::{bail, Context, Result};
use jsonrpsee::server::ServerHandle;
use reth_ipc::server::Builder;
use serde::{Deserialize, Serialize};
use serde_json::{self as encoder, Value};
use std::collections::HashMap;
use std::marker::Unpin;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufStream};
use tokio::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};

use super::rpc::{ZinitLoggingApiServer, ZinitRpcApiServer, ZinitServiceApiServer, ZinitSystemApiServer};

// JSON-RPC 2.0 structures
#[derive(Debug, Deserialize, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Deserialize, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

// JSON-RPC error codes


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
}

impl Api {
    pub fn new(zinit: ZInit) -> Api {
        Api {
            zinit,
        }
    }

    pub async fn serve(&self, endpoint: String) -> Result<ApiServer> {

        let server = Builder::default().build(endpoint);
        let mut module = ZinitRpcApiServer::into_rpc(self.clone());
        module.merge(ZinitSystemApiServer::into_rpc(self.clone()))?;
        module.merge(ZinitServiceApiServer::into_rpc(self.clone()))?;
        module.merge(ZinitLoggingApiServer::into_rpc(self.clone()))?;


        let _handle = server.start(module).await?;

        // TODO: ipv van server nen ipcserver, da moet gewoon nen jsonrpsee server buidler (voor http  +ws ) en die spawnt 
        // TODO: niewwe handle, terug alles merges, 
        // let server_rpc = jsonrpsee::server::ServerBuilder::default().build()

        
        Ok(ApiServer { _handle })
    }
}