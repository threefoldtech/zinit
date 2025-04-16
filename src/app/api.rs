use crate::zinit::{config, ZInit};
use anyhow::{bail, Context, Result};
use axum::{
    body::Bytes,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{options, post},
    Router,
};
use nix::sys::signal;
use serde::{Deserialize, Serialize};
use serde_json::{self as encoder, Value};
use std::collections::HashMap;
use std::marker::Unpin;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufStream};
use tokio::net::{TcpListener, UnixListener, UnixStream};

// Include the OpenRPC specification
const OPENRPC_SPEC: &str = include_str!("../../openrpc.json");

// JSON-RPC 2.0 structures
#[derive(Debug, Deserialize, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

// Type alias for batch requests
type JsonRpcBatchRequest = Vec<JsonRpcRequest>;

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

pub struct Api {
    zinit: ZInit,
    socket: PathBuf,
    // http_port removed as it's now in a separate binary
}

impl Api {
    pub fn new<P: AsRef<Path>>(zinit: ZInit, socket: P) -> Api {
        Api {
            zinit,
            socket: socket.as_ref().to_path_buf(),
        }
    }

    pub async fn serve(&self) -> Result<()> {
        // Start Unix socket server
        let listener = UnixListener::bind(&self.socket).context("failed to listen for socket")?;
        loop {
            if let Ok((stream, _addr)) = listener.accept().await {
                tokio::spawn(Self::handle(stream, self.zinit.clone()));
            }
        }
    }

    async fn handle(stream: UnixStream, zinit: ZInit) {
        let mut stream = BufStream::new(stream);
        let mut buffer = Vec::new();

        // Read the JSON-RPC request
        let mut temp_buf = [0u8; 1024];
        loop {
            match stream.read(&mut temp_buf).await {
                Ok(0) => break, // Connection closed
                Ok(n) => {
                    buffer.extend_from_slice(&temp_buf[..n]);

                    // Check if we have a complete JSON object/array
                    // This is a simple heuristic - we count opening and closing braces/brackets
                    let mut open_braces = 0;
                    let mut open_brackets = 0;
                    let mut in_string = false;
                    let mut escape_next = false;

                    for &b in &buffer {
                        if escape_next {
                            escape_next = false;
                            continue;
                        }

                        match b {
                            b'\\' if in_string => escape_next = true,
                            b'"' => in_string = !in_string,
                            b'{' if !in_string => open_braces += 1,
                            b'}' if !in_string => {
                                if open_braces > 0 {
                                    open_braces -= 1;
                                }
                            }
                            b'[' if !in_string => open_brackets += 1,
                            b']' if !in_string => {
                                if open_brackets > 0 {
                                    open_brackets -= 1;
                                }
                            }
                            _ => {}
                        }
                    }

                    // If all braces/brackets are balanced, we have a complete JSON
                    if open_braces == 0 && open_brackets == 0 && !in_string {
                        break;
                    }

                    // Safety check to prevent buffer from growing too large
                    if buffer.len() > 1024 * 1024 {
                        // 1MB limit
                        error!("request too large");
                        let _ = stream.write_all("request too large".as_ref()).await;
                        return;
                    }
                }
                Err(err) => {
                    error!("failed to read request: {}", err);
                    let _ = stream.write_all("bad request".as_ref()).await;
                    return;
                }
            }
        }

        // First, try to parse as a batch request
        let batch_request: Result<JsonRpcBatchRequest, _> = encoder::from_slice(&buffer);

        match batch_request {
            Ok(requests) if !requests.is_empty() => {
                // Valid batch request
                let mut responses = Vec::with_capacity(requests.len());

                for req in requests {
                    let response = if req.jsonrpc != "2.0" {
                        // Invalid JSON-RPC version
                        JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id: req.id.unwrap_or(Value::Null),
                            result: None,
                            error: Some(JsonRpcError {
                                code: INVALID_REQUEST,
                                message: "Invalid JSON-RPC version, expected 2.0".to_string(),
                                data: None,
                            }),
                        }
                    } else {
                        // Process the request
                        Self::process_jsonrpc(req, zinit.clone()).await
                    };

                    responses.push(response);
                }

                // Serialize and send the batch response
                let value = match encoder::to_vec(&responses) {
                    Ok(value) => value,
                    Err(err) => {
                        error!("failed to serialize batch response: {}", err);
                        return;
                    }
                };

                if let Err(err) = stream.write_all(&value).await {
                    error!("failed to send batch response to client: {}", err);
                };

                // Add a newline character to indicate the end of the response
                if let Err(err) = stream.write_all(b"\n").await {
                    error!("failed to send newline: {}", err);
                };

                let _ = stream.flush().await;
            }
            Ok(_) => {
                // Empty batch, return error
                let response = JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: Value::Null,
                    result: None,
                    error: Some(JsonRpcError {
                        code: INVALID_REQUEST,
                        message: "Invalid Request: Empty batch".to_string(),
                        data: None,
                    }),
                };

                let value = match encoder::to_vec(&response) {
                    Ok(value) => value,
                    Err(err) => {
                        error!("failed to serialize response: {}", err);
                        return;
                    }
                };

                if let Err(err) = stream.write_all(&value).await {
                    error!("failed to send response to client: {}", err);
                };

                // Add a newline character to indicate the end of the response
                if let Err(err) = stream.write_all(b"\n").await {
                    error!("failed to send newline: {}", err);
                };

                let _ = stream.flush().await;
            }
            Err(_) => {
                // Not a batch request, try as a single request
                let request: Result<JsonRpcRequest, _> = encoder::from_slice(&buffer);

                match request {
                    Ok(req) => {
                        // Valid JSON-RPC request
                        let response = if req.jsonrpc != "2.0" {
                            // Invalid JSON-RPC version
                            JsonRpcResponse {
                                jsonrpc: "2.0".to_string(),
                                id: req.id.unwrap_or(Value::Null),
                                result: None,
                                error: Some(JsonRpcError {
                                    code: INVALID_REQUEST,
                                    message: "Invalid JSON-RPC version, expected 2.0".to_string(),
                                    data: None,
                                }),
                            }
                        } else {
                            // Process the request
                            Self::process_jsonrpc(req, zinit).await
                        };

                        // Serialize and send the response
                        let value = match encoder::to_vec(&response) {
                            Ok(value) => value,
                            Err(err) => {
                                error!("failed to serialize response: {}", err);
                                return;
                            }
                        };

                        if let Err(err) = stream.write_all(&value).await {
                            error!("failed to send response to client: {}", err);
                        };

                        // Add a newline character to indicate the end of the response
                        if let Err(err) = stream.write_all(b"\n").await {
                            error!("failed to send newline: {}", err);
                        };

                        let _ = stream.flush().await;
                    }
                    Err(_) => {
                        // Invalid JSON, return error
                        let response = JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id: Value::Null,
                            result: None,
                            error: Some(JsonRpcError {
                                code: INVALID_REQUEST,
                                message: "Invalid Request: Could not parse JSON".to_string(),
                                data: None,
                            }),
                        };

                        let value = match encoder::to_vec(&response) {
                            Ok(value) => value,
                            Err(err) => {
                                error!("failed to serialize response: {}", err);
                                return;
                            }
                        };

                        if let Err(err) = stream.write_all(&value).await {
                            error!("failed to send response to client: {}", err);
                        };

                        // Add a newline character to indicate the end of the response
                        if let Err(err) = stream.write_all(b"\n").await {
                            error!("failed to send newline: {}", err);
                        };

                        let _ = stream.flush().await;
                    }
                }
            }
        }
    }

    // Process a JSON-RPC request and return a JSON-RPC response
    async fn process_jsonrpc(request: JsonRpcRequest, zinit: ZInit) -> JsonRpcResponse {
        let id = request.id.unwrap_or(Value::Null);

        // Process the request based on the method
        let result = match request.method.as_str() {
            // Obtain the OpenRPC specification
            "rpc.discover" => {
                let spec: Value = match serde_json::from_str(OPENRPC_SPEC) {
                    Ok(value) => value,
                    Err(err) => {
                        return JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id,
                            result: None,
                            error: Some(JsonRpcError {
                                code: INTERNAL_ERROR,
                                message: format!("Failed to parse OpenRPC spec: {}", err),
                                data: None,
                            }),
                        };
                    }
                };
                Ok(spec)
            }

            // Service management methods
            "service.list" => Self::list(zinit).await,

            "service.status" => {
                if let Some(params) = &request.params {
                    if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                        Self::status(name, zinit).await
                    } else {
                        Err(anyhow::anyhow!("Missing or invalid 'name' parameter"))
                    }
                } else {
                    Err(anyhow::anyhow!("Missing parameters"))
                }
            }

            "service.start" => {
                if let Some(params) = &request.params {
                    if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                        Self::start(name, zinit).await
                    } else {
                        Err(anyhow::anyhow!("Missing or invalid 'name' parameter"))
                    }
                } else {
                    Err(anyhow::anyhow!("Missing parameters"))
                }
            }

            "service.stop" => {
                if let Some(params) = &request.params {
                    if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                        Self::stop(name, zinit).await
                    } else {
                        Err(anyhow::anyhow!("Missing or invalid 'name' parameter"))
                    }
                } else {
                    Err(anyhow::anyhow!("Missing parameters"))
                }
            }

            "service.monitor" => {
                if let Some(params) = &request.params {
                    if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                        Self::monitor(name, zinit).await
                    } else {
                        Err(anyhow::anyhow!("Missing or invalid 'name' parameter"))
                    }
                } else {
                    Err(anyhow::anyhow!("Missing parameters"))
                }
            }

            "service.forget" => {
                if let Some(params) = &request.params {
                    if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                        Self::forget(name, zinit).await
                    } else {
                        Err(anyhow::anyhow!("Missing or invalid 'name' parameter"))
                    }
                } else {
                    Err(anyhow::anyhow!("Missing parameters"))
                }
            }

            "service.kill" => {
                if let Some(params) = &request.params {
                    if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                        if let Some(signal) = params.get("signal").and_then(|v| v.as_str()) {
                            Self::kill(name, signal, zinit).await
                        } else {
                            Err(anyhow::anyhow!("Missing or invalid 'signal' parameter"))
                        }
                    } else {
                        Err(anyhow::anyhow!("Missing or invalid 'name' parameter"))
                    }
                } else {
                    Err(anyhow::anyhow!("Missing parameters"))
                }
            }

            // System operations
            "system.shutdown" => Self::shutdown(zinit).await,
            "system.reboot" => Self::reboot(zinit).await,

            // Service file operations
            "service.create" => {
                if let Some(params) = &request.params {
                    if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                        if let Some(content) = params.get("content").and_then(|v| v.as_object()) {
                            Self::create_service(name, content, zinit).await
                        } else {
                            Err(anyhow::anyhow!("Missing or invalid 'content' parameter"))
                        }
                    } else {
                        Err(anyhow::anyhow!("Missing or invalid 'name' parameter"))
                    }
                } else {
                    Err(anyhow::anyhow!("Missing parameters"))
                }
            }

            "service.delete" => {
                if let Some(params) = &request.params {
                    if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                        Self::delete_service(name, zinit).await
                    } else {
                        Err(anyhow::anyhow!("Missing or invalid 'name' parameter"))
                    }
                } else {
                    Err(anyhow::anyhow!("Missing parameters"))
                }
            }

            "service.get" => {
                if let Some(params) = &request.params {
                    if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                        Self::get_service(name, zinit).await
                    } else {
                        Err(anyhow::anyhow!("Missing or invalid 'name' parameter"))
                    }
                } else {
                    Err(anyhow::anyhow!("Missing parameters"))
                }
            }

            // Unknown method
            _ => Err(anyhow::anyhow!("Method not found: {}", request.method)),
        };

        // Create the response
        match result {
            Ok(value) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(value),
                error: None,
            },
            Err(err) => {
                // Map error messages to specific error codes
                let code = if err.to_string().contains("Method not found") {
                    METHOD_NOT_FOUND
                } else if err.to_string().contains("Missing or invalid") {
                    INVALID_PARAMS
                } else if err.to_string().contains("service name")
                    && err.to_string().contains("unknown")
                {
                    SERVICE_NOT_FOUND
                } else if err.to_string().contains("already monitored") {
                    SERVICE_ALREADY_MONITORED
                } else if err.to_string().contains("service") && err.to_string().contains("is up") {
                    SERVICE_IS_UP
                } else if err.to_string().contains("service") && err.to_string().contains("is down")
                {
                    SERVICE_IS_DOWN
                } else if err.to_string().contains("signal") {
                    INVALID_SIGNAL
                } else if err.to_string().contains("config") {
                    CONFIG_ERROR
                } else if err.to_string().contains("shutting down") {
                    SHUTTING_DOWN
                } else if err.to_string().contains("Service")
                    && err.to_string().contains("already exists")
                {
                    SERVICE_ALREADY_EXISTS
                } else if err.to_string().contains("Failed to")
                    && (err.to_string().contains("service file")
                        || err.to_string().contains("configuration"))
                {
                    SERVICE_FILE_ERROR
                } else {
                    INTERNAL_ERROR
                };

                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: None,
                    error: Some(JsonRpcError {
                        code,
                        message: err.to_string(),
                        data: None,
                    }),
                }
            }
        }
    }


    async fn log(stream: &mut BufStream<UnixStream>, zinit: ZInit, follow: bool) -> Result<Value> {
        let mut logs = zinit.logs(follow).await;

        while let Some(line) = logs.recv().await {
            stream.write_all(line.as_bytes()).await?;
            stream.write_all(b"\n").await?;
            stream.flush().await?;
        }

        Ok(Value::Null)
    }

    async fn list(zinit: ZInit) -> Result<Value> {
        let services = zinit.list().await?;
        let mut map: HashMap<String, String> = HashMap::new();
        for service in services {
            let state = zinit.status(&service).await?;
            map.insert(service, format!("{:?}", state.state));
        }

        Ok(encoder::to_value(map)?)
    }

    async fn monitor<S: AsRef<str>>(name: S, zinit: ZInit) -> Result<Value> {
        let (name, service) = config::load(format!("{}.yaml", name.as_ref()))
            .context("failed to load service config")?;
        zinit.monitor(name, service).await?;
        Ok(Value::Null)
    }

    async fn forget<S: AsRef<str>>(name: S, zinit: ZInit) -> Result<Value> {
        zinit.forget(name).await?;
        Ok(Value::Null)
    }

    async fn stop<S: AsRef<str>>(name: S, zinit: ZInit) -> Result<Value> {
        zinit.stop(name).await?;
        Ok(Value::Null)
    }

    async fn shutdown(zinit: ZInit) -> Result<Value> {
        tokio::spawn(async move {
            if let Err(err) = zinit.shutdown().await {
                error!("failed to execute shutdown: {}", err);
            }
        });

        Ok(Value::Null)
    }

    async fn reboot(zinit: ZInit) -> Result<Value> {
        tokio::spawn(async move {
            if let Err(err) = zinit.reboot().await {
                error!("failed to execute reboot: {}", err);
            }
        });

        Ok(Value::Null)
    }

    async fn start<S: AsRef<str>>(name: S, zinit: ZInit) -> Result<Value> {
        zinit.start(name).await?;
        Ok(Value::Null)
    }

    async fn kill<S: AsRef<str>>(name: S, sig: S, zinit: ZInit) -> Result<Value> {
        let sig = sig.as_ref();
        let sig = signal::Signal::from_str(&sig.to_uppercase())?;
        zinit.kill(name, sig).await?;
        Ok(Value::Null)
    }

    async fn status<S: AsRef<str>>(name: S, zinit: ZInit) -> Result<Value> {
        let status = zinit.status(&name).await?;

        let result = Status {
            name: name.as_ref().into(),
            pid: status.pid.as_raw() as u32,
            state: format!("{:?}", status.state),
            target: format!("{:?}", status.target),
            after: {
                let mut after = HashMap::new();
                for service in status.service.after {
                    let status = match zinit.status(&service).await {
                        Ok(dep) => dep.state,
                        Err(_) => crate::zinit::State::Unknown,
                    };
                    after.insert(service, format!("{:?}", status));
                }
                after
            },
        };

        Ok(encoder::to_value(result)?)
    }

    // THESE FUNCTIONS ARE FOR SERVICE FILE MANAGEMENT AND THUS NOT DIRECTLY RELATED TO THE ZINIT INSTANCE
    // --> thats why the zinit argument is not necessary here! --> TODO: remove it!
    async fn create_service<S: AsRef<str>>(
        name: S,
        content: &serde_json::Map<String, Value>,
        zinit: ZInit,
    ) -> Result<Value> {
        use std::fs;
        use std::io::Write;

        let name = name.as_ref();

        // Validate service name (no path traversal, valid characters)
        if name.contains('/') || name.contains('\\') || name.contains('.') {
            bail!("Invalid service name: must not contain '/', '\\', or '.'");
        }

        // Construct the file path
        let file_path = PathBuf::from(format!("{}.yaml", name));

        // Check if the service file already exists
        if file_path.exists() {
            bail!("Service '{}' already exists", name);
        }

        // Convert the JSON content to YAML
        let yaml_content = serde_yaml::to_string(content)
            .context("Failed to convert service configuration to YAML")?;

        // Write the YAML content to the file
        let mut file = fs::File::create(&file_path).context("Failed to create service file")?;
        file.write_all(yaml_content.as_bytes())
            .context("Failed to write service configuration")?;

        Ok(Value::String(format!(
            "Service '{}' created successfully",
            name
        )))
    }
    async fn delete_service<S: AsRef<str>>(name: S, zinit: ZInit) -> Result<Value> {
        use std::fs;

        let name = name.as_ref();

        // Validate service name (no path traversal, valid characters)
        if name.contains('/') || name.contains('\\') || name.contains('.') {
            bail!("Invalid service name: must not contain '/', '\\', or '.'");
        }

        // Construct the file path
        let file_path = PathBuf::from(format!("{}.yaml", name));

        // Check if the service file exists
        if !file_path.exists() {
            bail!("Service '{}' not found", name);
        }

        // Delete the file
        fs::remove_file(&file_path).context("Failed to delete service file")?;

        Ok(Value::String(format!(
            "Service '{}' deleted successfully",
            name
        )))
    }
    async fn get_service<S: AsRef<str>>(name: S, zinit: ZInit) -> Result<Value> {
        use std::fs;

        let name = name.as_ref();

        // Validate service name (no path traversal, valid characters)
        if name.contains('/') || name.contains('\\') || name.contains('.') {
            bail!("Invalid service name: must not contain '/', '\\', or '.'");
        }

        // Construct the file path
        let file_path = PathBuf::from(format!("{}.yaml", name));

        // Check if the service file exists
        if !file_path.exists() {
            bail!("Service '{}' not found", name);
        }

        // Read the file content
        let yaml_content = fs::read_to_string(&file_path).context("Failed to read service file")?;

        // Parse YAML to JSON
        let yaml_value: serde_yaml::Value =
            serde_yaml::from_str(&yaml_content).context("Failed to parse YAML content")?;

        // Convert YAML value to JSON value
        let json_value =
            serde_json::to_value(yaml_value).context("Failed to convert YAML to JSON")?;

        Ok(json_value)
    }
}

use std::sync::atomic::{AtomicU64, Ordering};

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
        // The server sends a JSON response followed by a newline character
        // We need to read the entire response until we find the terminating newline
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

    // Keep the original command method for backward compatibility if needed
    #[allow(dead_code)]
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
