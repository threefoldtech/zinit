use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{self as encoder, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufStream};
use tokio::net::UnixStream;

use super::server::{Status, ZinitResponse, ZinitState};

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

pub struct Client {
    socket: PathBuf,
    next_id: AtomicU64,
}

impl Client {
    pub fn new<P: AsRef<Path>>(socket: P) -> Client {
        Client {
            socket: socket.as_ref().to_path_buf(),
            next_id: AtomicU64::new(1),
        }
    }

    async fn connect(&self) -> Result<UnixStream> {
        UnixStream::connect(&self.socket).await.with_context(|| {
            format!(
                "failed to connect to '{:?}'. is zinit listening on that socket?",
                self.socket
            )
        })
    }

    // Send a JSON-RPC request and return the result
    async fn jsonrpc_request(&self, method: &str, params: Option<Value>) -> Result<Value> {
        // First try JSON-RPC
        let result = self.try_jsonrpc(method, params.clone()).await;

        // If JSON-RPC fails, try legacy protocol
        if let Err(e) = &result {
            if e.to_string().contains("Invalid JSON-RPC response")
                || e.to_string().contains("Failed to parse")
            {
                debug!("JSON-RPC failed, trying legacy protocol: {}", e);
                return self.try_legacy_protocol(method, params).await;
            }
        }

        result
    }

    // Try using JSON-RPC protocol
    async fn try_jsonrpc(&self, method: &str, params: Option<Value>) -> Result<Value> {
        // Get a unique ID for this request
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::Number(serde_json::Number::from(id))),
            method: method.to_string(),
            params,
        };

        let mut con = BufStream::new(self.connect().await?);

        // Serialize and send the request
        let request_bytes = encoder::to_vec(&request)?;
        con.write_all(&request_bytes).await?;
        con.flush().await?;

        // Read and parse the response
        let mut buffer = Vec::new();
        let mut temp_buf = [0u8; 1024];

        loop {
            let n = con.read(&mut temp_buf).await?;
            if n == 0 {
                break; // Connection closed
            }

            buffer.extend_from_slice(&temp_buf[..n]);

            // Check if the buffer ends with a newline
            if buffer.ends_with(b"\n") {
                break;
            }
        }

        // Convert to string and trim the trailing newline
        let data = String::from_utf8(buffer)?;
        let data = data.trim_end();

        // Parse the JSON-RPC response
        let response: JsonRpcResponse = encoder::from_str(data)?;

        // Handle the response
        if let Some(error) = response.error {
            bail!("RPC error ({}): {}", error.code, error.message);
        } else if let Some(result) = response.result {
            Ok(result)
        } else {
            bail!("Invalid JSON-RPC response: missing both result and error");
        }
    }

    // Try to use the legacy protocol as a fallback
    async fn try_legacy_protocol(&self, method: &str, params: Option<Value>) -> Result<Value> {
        // Convert JSON-RPC method and params to legacy command
        let cmd = match method {
            "service.list" => "list".to_string(),
            "system.shutdown" => "shutdown".to_string(),
            "system.reboot" => "reboot".to_string(),
            "service.status" => {
                if let Some(params) = params {
                    if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                        format!("status {}", name)
                    } else {
                        bail!("Missing or invalid 'name' parameter");
                    }
                } else {
                    bail!("Missing parameters");
                }
            }
            "service.start" => {
                if let Some(params) = params {
                    if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                        format!("start {}", name)
                    } else {
                        bail!("Missing or invalid 'name' parameter");
                    }
                } else {
                    bail!("Missing parameters");
                }
            }
            "service.stop" => {
                if let Some(params) = params {
                    if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                        format!("stop {}", name)
                    } else {
                        bail!("Missing or invalid 'name' parameter");
                    }
                } else {
                    bail!("Missing parameters");
                }
            }
            "service.forget" => {
                if let Some(params) = params {
                    if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                        format!("forget {}", name)
                    } else {
                        bail!("Missing or invalid 'name' parameter");
                    }
                } else {
                    bail!("Missing parameters");
                }
            }
            "service.monitor" => {
                if let Some(params) = params {
                    if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                        format!("monitor {}", name)
                    } else {
                        bail!("Missing or invalid 'name' parameter");
                    }
                } else {
                    bail!("Missing parameters");
                }
            }
            "service.kill" => {
                if let Some(params) = params {
                    if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                        if let Some(signal) = params.get("signal").and_then(|v| v.as_str()) {
                            format!("kill {} {}", name, signal)
                        } else {
                            bail!("Missing or invalid 'signal' parameter");
                        }
                    } else {
                        bail!("Missing or invalid 'name' parameter");
                    }
                } else {
                    bail!("Missing parameters");
                }
            }
            "service.create" => {
                // The legacy protocol doesn't directly support service creation
                bail!("Service creation not supported in legacy protocol");
            }
            "service.delete" => {
                // The legacy protocol doesn't directly support service deletion
                bail!("Service deletion not supported in legacy protocol");
            }
            "service.get" => {
                // The legacy protocol doesn't directly support getting service config
                bail!("Getting service configuration not supported in legacy protocol");
            }
            "rpc.discover" => {
                // This is a JSON-RPC specific method with no legacy equivalent
                bail!("RPC discovery not supported in legacy protocol");
            }
            "service.restart" => {
                if let Some(params) = params {
                    if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                        format!("restart {}", name)
                    } else {
                        bail!("Missing or invalid 'name' parameter");
                    }
                } else {
                    bail!("Missing parameters");
                }
            }
            "log" => {
                if let Some(params) = params {
                    if let Some(filter) = params.get("filter").and_then(|v| v.as_str()) {
                        if let Some(snapshot) = params.get("snapshot").and_then(|v| v.as_bool()) {
                            if snapshot {
                                format!("log snapshot {}", filter)
                            } else {
                                format!("log {}", filter)
                            }
                        } else {
                            format!("log {}", filter)
                        }
                    } else {
                        "log".to_string()
                    }
                } else {
                    "log".to_string()
                }
            }
            _ => bail!("Unsupported method for legacy protocol: {}", method),
        };

        // Use the command method to send the legacy command
        self.command(&cmd).await
    }

    // Command method for the legacy protocol
    async fn command(&self, c: &str) -> Result<Value> {
        let mut con = BufStream::new(self.connect().await?);

        let _ = con.write(c.as_bytes()).await?;
        let _ = con.write(b"\n").await?;
        con.flush().await?;

        let mut buffer = Vec::new();
        let mut temp_buf = [0u8; 1024];

        loop {
            let n = con.read(&mut temp_buf).await?;
            if n == 0 {
                break; // Connection closed
            }

            buffer.extend_from_slice(&temp_buf[..n]);

            // Check if the buffer ends with a newline
            if buffer.ends_with(b"\n") {
                break;
            }
        }

        // Convert to string and trim the trailing newline
        let data = String::from_utf8(buffer)?;
        let data = data.trim_end();
        let response: ZinitResponse = encoder::from_str(data)?;

        match response.state {
            ZinitState::Ok => Ok(response.body),
            ZinitState::Error => {
                let err: String = encoder::from_value(response.body)?;
                bail!(err)
            }
        }
    }

    pub async fn logs<O: tokio::io::AsyncWrite + Unpin, S: AsRef<str>>(
        &self,
        mut out: O,
        filter: Option<S>,
        follow: bool,
    ) -> Result<()> {
        // For now, keep using the original log command since it's a special case
        let mut con = self.connect().await?;
        if follow {
            // default behavior of log with no extra arguments
            // is to stream all logs
            con.write_all(b"log\n").await?;
        } else {
            // adding a snapshot subcmd will make it auto terminate
            // immediate after
            con.write_all(b"log snapshot\n").await?;
        }
        con.flush().await?;
        match filter {
            None => tokio::io::copy(&mut con, &mut out).await?,
            Some(filter) => {
                let filter = format!("{}:", filter.as_ref());
                let mut stream = BufStream::new(con);
                loop {
                    let mut line = String::new();
                    match stream.read_line(&mut line).await {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(err) => {
                            bail!("failed to read stream: {}", err);
                        }
                    }

                    if line[4..].starts_with(&filter) {
                        let _ = out.write_all(line.as_bytes()).await;
                    }
                }
                0
            }
        };

        Ok(())
    }

    pub async fn list(&self) -> Result<HashMap<String, String>> {
        let response = self.jsonrpc_request("service.list", None).await?;
        Ok(encoder::from_value(response)?)
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.jsonrpc_request("system.shutdown", None).await?;
        Ok(())
    }

    pub async fn reboot(&self) -> Result<()> {
        self.jsonrpc_request("system.reboot", None).await?;
        Ok(())
    }

    pub async fn status<S: AsRef<str>>(&self, name: S) -> Result<Status> {
        let params = serde_json::json!({
            "name": name.as_ref()
        });

        let response = self.jsonrpc_request("service.status", Some(params)).await?;
        Ok(encoder::from_value(response)?)
    }

    pub async fn start<S: AsRef<str>>(&self, name: S) -> Result<()> {
        let params = serde_json::json!({
            "name": name.as_ref()
        });

        self.jsonrpc_request("service.start", Some(params)).await?;
        Ok(())
    }

    pub async fn stop<S: AsRef<str>>(&self, name: S) -> Result<()> {
        let params = serde_json::json!({
            "name": name.as_ref()
        });

        self.jsonrpc_request("service.stop", Some(params)).await?;
        Ok(())
    }

    pub async fn forget<S: AsRef<str>>(&self, name: S) -> Result<()> {
        let params = serde_json::json!({
            "name": name.as_ref()
        });

        self.jsonrpc_request("service.forget", Some(params)).await?;
        Ok(())
    }

    pub async fn monitor<S: AsRef<str>>(&self, name: S) -> Result<()> {
        let params = serde_json::json!({
            "name": name.as_ref(),
        });

        self.jsonrpc_request("service.monitor", Some(params))
            .await?;
        Ok(())
    }

    pub async fn kill<S: AsRef<str>>(&self, name: S, sig: S) -> Result<()> {
        let params = serde_json::json!({
            "name": name.as_ref(),
            "signal": sig.as_ref()
        });

        self.jsonrpc_request("service.kill", Some(params)).await?;
        Ok(())
    }

    // Service file operations
    pub async fn create_service<S: AsRef<str>>(
        &self,
        name: S,
        content: serde_json::Map<String, Value>,
    ) -> Result<String> {
        let params = serde_json::json!({
            "name": name.as_ref(),
            "content": content
        });

        let response = self.jsonrpc_request("service.create", Some(params)).await?;
        match response {
            Value::String(s) => Ok(s),
            _ => Ok("Service created successfully".to_string()),
        }
    }

    pub async fn delete_service<S: AsRef<str>>(&self, name: S) -> Result<String> {
        let params = serde_json::json!({
            "name": name.as_ref()
        });

        let response = self.jsonrpc_request("service.delete", Some(params)).await?;
        match response {
            Value::String(s) => Ok(s),
            _ => Ok("Service deleted successfully".to_string()),
        }
    }

    pub async fn get_service<S: AsRef<str>>(&self, name: S) -> Result<Value> {
        let params = serde_json::json!({
            "name": name.as_ref()
        });

        self.jsonrpc_request("service.get", Some(params)).await
    }
}
