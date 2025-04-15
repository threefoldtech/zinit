use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{self as encoder, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufStream};
use tokio::net::UnixStream;

use super::server::Status;

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
        self.try_jsonrpc(method, params).await
    }

    // Use JSON-RPC protocol
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

        // Parse the JSON-RPC response - improved error handling
        let response: JsonRpcResponse = match encoder::from_str(data) {
            Ok(response) => response,
            Err(e) => {
                // If we can't parse the response as JSON-RPC, this is an error
                error!("Failed to parse JSON-RPC response: {}", e);
                error!("Raw response: {}", data);
                bail!("Invalid response from server: failed to parse JSON-RPC")
            }
        };

        // Handle the response according to JSON-RPC 2.0 spec
        if let Some(error) = response.error {
            // Error response - has 'error' field
            bail!("RPC error ({}): {}", error.code, error.message);
        } else {
            // Success response - return result (which might be null)
            // This properly handles all success responses including null values
            Ok(response.result.unwrap_or(Value::Null))
        }
    }

    // JSON-RPC is now the only supported protocol
    // Legacy command method has been removed

    pub async fn logs<O: tokio::io::AsyncWrite + Unpin, S: AsRef<str>>(
        &self,
        mut out: O,
        filter: Option<S>,
        follow: bool,
    ) -> Result<()> {
        // Convert to JSON-RPC params
        let params = if let Some(ref filter) = filter {
            serde_json::json!({
                "filter": filter.as_ref(),
                "snapshot": !follow
            })
        } else {
            serde_json::json!({
                "snapshot": !follow
            })
        };

        // Make a JSON-RPC request to the log method
        let response = self.jsonrpc_request("log", Some(params)).await?;

        // Write the log data to the output
        if let Some(logs) = response.as_array() {
            for log in logs {
                if let Some(log_str) = log.as_str() {
                    out.write_all(log_str.as_bytes()).await?;
                    out.write_all(b"\n").await?;
                }
            }
        } else if let Some(log_str) = response.as_str() {
            out.write_all(log_str.as_bytes()).await?;
        }

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
