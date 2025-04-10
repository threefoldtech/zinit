//! Zinit Client Library
//!
//! A simple client library for interacting with Zinit process manager.
//! Supports both Unix socket and HTTP transport methods.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufStream};
use tokio::net::UnixStream;

/// Errors that can occur when using the Zinit client
#[derive(Error, Debug)]
pub enum ClientError {
    #[error("Failed to connect to Zinit: {0}")]
    ConnectionError(String),

    #[error("Invalid response from Zinit: {0}")]
    InvalidResponse(String),

    #[error("RPC error ({0}): {1}")]
    RpcError(i32, String),

    #[error("Service not found: {0}")]
    ServiceNotFound(String),

    #[error("HTTP error: {0}")]
    HttpError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),
}

/// JSON-RPC request structure
#[derive(Debug, Deserialize, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

/// JSON-RPC response structure
#[derive(Debug, Deserialize, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

/// JSON-RPC error structure
#[derive(Debug, Deserialize, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

/// Service status information
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Status {
    pub name: String,
    pub pid: u32,
    pub state: String,
    pub target: String,
    pub after: HashMap<String, String>,
}

/// Transport method for communicating with Zinit
pub enum Transport {
    /// Unix socket transport
    UnixSocket(PathBuf),
    /// HTTP transport
    Http(String),
}

/// Zinit client for interacting with the Zinit process manager
pub struct Client {
    transport: Transport,
    next_id: AtomicU64,
}

impl Client {
    /// Create a new client using Unix socket transport
    pub fn unix_socket<P: AsRef<Path>>(socket_path: P) -> Self {
        Client {
            transport: Transport::UnixSocket(socket_path.as_ref().to_path_buf()),
            next_id: AtomicU64::new(1),
        }
    }

    /// Create a new client using HTTP transport
    pub fn http<S: Into<String>>(url: S) -> Self {
        Client {
            transport: Transport::Http(url.into()),
            next_id: AtomicU64::new(1),
        }
    }

    /// Send a JSON-RPC request and return the result
    async fn jsonrpc_request(&self, method: &str, params: Option<Value>) -> Result<Value, ClientError> {
        // Get a unique ID for this request
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::Number(serde_json::Number::from(id))),
            method: method.to_string(),
            params,
        };

        match &self.transport {
            Transport::UnixSocket(socket_path) => self.unix_socket_request(&request, socket_path).await,
            Transport::Http(url) => self.http_request(&request, url).await,
        }
    }

    /// Send a request via Unix socket
    async fn unix_socket_request(
        &self,
        request: &JsonRpcRequest,
        socket_path: &Path,
    ) -> Result<Value, ClientError> {
        // Connect to the Unix socket
        let stream = UnixStream::connect(socket_path)
            .await
            .with_context(|| {
                format!(
                    "Failed to connect to '{:?}'. Is Zinit listening on that socket?",
                    socket_path
                )
            })
            .map_err(|e| ClientError::ConnectionError(e.to_string()))?;

        let mut con = BufStream::new(stream);

        // Serialize and send the request
        let request_bytes = serde_json::to_vec(request)?;
        con.write_all(&request_bytes).await?;
        con.write_all(b"\n").await?;
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
        let data = String::from_utf8(buffer)
            .map_err(|e| ClientError::InvalidResponse(e.to_string()))?;
        let data = data.trim_end();

        // Parse the JSON-RPC response
        let response: JsonRpcResponse = serde_json::from_str(data)?;

        // Handle the response
        if let Some(error) = response.error {
            return Err(ClientError::RpcError(error.code, error.message));
        } else if let Some(result) = response.result {
            Ok(result)
        } else {
            Err(ClientError::InvalidResponse(
                "Missing both result and error".to_string(),
            ))
        }
    }

    /// Send a request via HTTP
    async fn http_request(
        &self,
        request: &JsonRpcRequest,
        url: &str,
    ) -> Result<Value, ClientError> {
        // Create an HTTP client
        let client = reqwest::Client::new();

        // Send the request
        let response = client
            .post(url)
            .json(request)
            .send()
            .await
            .map_err(|e| ClientError::HttpError(e.to_string()))?;

        // Check if the response is successful
        if !response.status().is_success() {
            return Err(ClientError::HttpError(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        // Parse the response
        let json_response: JsonRpcResponse = response
            .json()
            .await
            .map_err(|e| ClientError::InvalidResponse(e.to_string()))?;

        // Handle the response
        if let Some(error) = json_response.error {
            return Err(ClientError::RpcError(error.code, error.message));
        } else if let Some(result) = json_response.result {
            Ok(result)
        } else {
            Err(ClientError::InvalidResponse(
                "Missing both result and error".to_string(),
            ))
        }
    }

    /// List all services and their states
    pub async fn list(&self) -> Result<HashMap<String, String>, ClientError> {
        let response = self.jsonrpc_request("service.list", None).await?;
        Ok(serde_json::from_value(response)?)
    }

    /// Get the status of a service
    pub async fn status<S: AsRef<str>>(&self, name: S) -> Result<Status, ClientError> {
        let params = serde_json::json!({
            "name": name.as_ref()
        });

        let response = self.jsonrpc_request("service.status", Some(params)).await?;
        Ok(serde_json::from_value(response)?)
    }

    /// Start a service
    pub async fn start<S: AsRef<str>>(&self, name: S) -> Result<(), ClientError> {
        let params = serde_json::json!({
            "name": name.as_ref()
        });

        self.jsonrpc_request("service.start", Some(params)).await?;
        Ok(())
    }

    /// Stop a service
    pub async fn stop<S: AsRef<str>>(&self, name: S) -> Result<(), ClientError> {
        let params = serde_json::json!({
            "name": name.as_ref()
        });

        self.jsonrpc_request("service.stop", Some(params)).await?;
        Ok(())
    }

    /// Restart a service
    pub async fn restart<S: AsRef<str>>(&self, name: S) -> Result<(), ClientError> {
        // First stop the service
        self.stop(name.as_ref()).await?;

        // Wait for the service to stop
        let mut attempts = 0;
        while attempts < 10 {
            match self.status(name.as_ref()).await {
                Ok(status) => {
                    if status.pid == 0 && status.target == "Down" {
                        // Service is stopped, now start it
                        return self.start(name.as_ref()).await;
                    }
                }
                Err(_) => {
                    // If we can't get status, try to start anyway
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            attempts += 1;
        }

        // Try to start the service even if it might not be fully stopped
        self.start(name.as_ref()).await
    }

    /// Forget a service
    pub async fn forget<S: AsRef<str>>(&self, name: S) -> Result<(), ClientError> {
        let params = serde_json::json!({
            "name": name.as_ref()
        });

        self.jsonrpc_request("service.forget", Some(params)).await?;
        Ok(())
    }

    /// Monitor a service
    pub async fn monitor<S: AsRef<str>>(&self, name: S) -> Result<(), ClientError> {
        let params = serde_json::json!({
            "name": name.as_ref()
        });

        self.jsonrpc_request("service.monitor", Some(params)).await?;
        Ok(())
    }

    /// Send a signal to a service
    pub async fn kill<S: AsRef<str>>(&self, name: S, signal: S) -> Result<(), ClientError> {
        let params = serde_json::json!({
            "name": name.as_ref(),
            "signal": signal.as_ref()
        });

        self.jsonrpc_request("service.kill", Some(params)).await?;
        Ok(())
    }

    /// Shutdown the system
    pub async fn shutdown(&self) -> Result<(), ClientError> {
        self.jsonrpc_request("system.shutdown", None).await?;
        Ok(())
    }

    /// Reboot the system
    pub async fn reboot(&self) -> Result<(), ClientError> {
        self.jsonrpc_request("system.reboot", None).await?;
        Ok(())
    }

    /// Create a new service
    pub async fn create_service<S: AsRef<str>>(
        &self,
        name: S,
        content: serde_json::Map<String, Value>,
    ) -> Result<String, ClientError> {
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

    /// Delete a service
    pub async fn delete_service<S: AsRef<str>>(&self, name: S) -> Result<String, ClientError> {
        let params = serde_json::json!({
            "name": name.as_ref()
        });

        let response = self.jsonrpc_request("service.delete", Some(params)).await?;
        match response {
            Value::String(s) => Ok(s),
            _ => Ok("Service deleted successfully".to_string()),
        }
    }

    /// Get a service configuration
    pub async fn get_service<S: AsRef<str>>(&self, name: S) -> Result<Value, ClientError> {
        let params = serde_json::json!({
            "name": name.as_ref()
        });

        self.jsonrpc_request("service.get", Some(params)).await
    }
}

#[cfg(test)]
mod tests {
    // Tests would go here
}