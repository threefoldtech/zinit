use crate::zinit::{config, ZInit};
use anyhow::{Context, Result};
use nix::sys::signal;
use serde::{Deserialize, Serialize};
use serde_json::{self as encoder, Value};
use std::collections::HashMap;
use std::marker::Unpin;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufStream};
use tokio::net::{UnixListener, UnixStream};

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

// Type alias for batch responses
type JsonRpcBatchResponse = Vec<JsonRpcResponse>;

#[derive(Debug, Deserialize, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

// JSON-RPC error codes
// Standard JSON-RPC error codes
const PARSE_ERROR: i32 = -32700;
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
struct Response {
    pub state: State,
    pub body: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum State {
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
}

impl Api {
    pub fn new<P: AsRef<Path>>(zinit: ZInit, socket: P) -> Api {
        Api {
            zinit,
            socket: socket.as_ref().to_path_buf(),
        }
    }

    pub async fn serve(&self) -> Result<()> {
        let listener = UnixListener::bind(&self.socket).context("failed to listen for socket")?;
        loop {
            if let Ok((stream, _addr)) = listener.accept().await {
                tokio::spawn(Self::handle(stream, self.zinit.clone()));
            }
        }
    }

    // Process a JSON-RPC request and return a JSON-RPC response
    async fn process_jsonrpc(request: JsonRpcRequest, zinit: ZInit) -> JsonRpcResponse {
        let id = request.id.unwrap_or(Value::Null);

        // Process the request based on the method
        let result = match request.method.as_str() {
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
            },

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
            },

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
            },

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
            },

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
            },

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
            },

            // System operations
            "system.shutdown" => Self::shutdown(zinit).await,
            "system.reboot" => Self::reboot(zinit).await,

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
                } else if err.to_string().contains("service name") && err.to_string().contains("unknown") {
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
            },
        }
    }

    async fn handle(stream: UnixStream, zinit: ZInit) {
        let mut stream = BufStream::new(stream);
        
        // First, try to read as a JSON-RPC request
        let mut buffer = Vec::new();
        
        // Read a small amount first to check if it's JSON
        let mut peek_buffer = [0u8; 1];
        match stream.read_exact(&mut peek_buffer).await {
            Ok(_) => {
                // Put the peeked byte back into the buffer
                buffer.push(peek_buffer[0]);
                
                // If it starts with '{', it's likely JSON-RPC
                if peek_buffer[0] == b'{' || peek_buffer[0] == b'[' {
                    // Read the request until we find a newline or a certain amount of data
                    // This avoids waiting for EOF which would require the client to close the connection
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
                                        },
                                        b'[' if !in_string => open_brackets += 1,
                                        b']' if !in_string => {
                                            if open_brackets > 0 {
                                                open_brackets -= 1;
                                            }
                                        },
                                        _ => {}
                                    }
                                }
                                
                                // If all braces/brackets are balanced, we have a complete JSON
                                if open_braces == 0 && open_brackets == 0 && !in_string {
                                    break;
                                }
                                
                                // Safety check to prevent buffer from growing too large
                                if buffer.len() > 1024 * 1024 { // 1MB limit
                                    error!("request too large");
                                    let _ = stream.write_all("request too large".as_ref()).await;
                                    return;
                                }
                            },
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
                            return;
                        },
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
                            return;
                        },
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
                                    return;
                                },
                                Err(_) => {
                                    // Not a valid JSON-RPC request, try the line-based protocol
                                }
                            }
                        }
                    }
                }
                
                // If we get here, it's not a JSON-RPC request
                // Try the line-based protocol
                let mut cmd = String::from_utf8_lossy(&buffer).to_string();
                
                // Read the rest of the line
                let mut line = String::new();
                if let Err(err) = stream.read_line(&mut line).await {
                    error!("failed to read command: {}", err);
                    let _ = stream.write_all("bad request".as_ref()).await;
                    return;
                }
                
                // Combine the peeked byte with the rest of the line
                cmd.push_str(&line);
                
                // Process using the old line-based protocol
                let response = match Self::process(cmd, &mut stream, zinit).await {
                    // When process returns None means we can terminate without
                    // writing any result to the socket.
                    Ok(None) => return,
                    Ok(Some(body)) => Response {
                        body,
                        state: State::Ok,
                    },
                    Err(err) => Response {
                        state: State::Error,
                        body: encoder::to_value(format!("{}", err)).unwrap(),
                    },
                };
                
                let value = match encoder::to_vec(&response) {
                    Ok(value) => value,
                    Err(err) => {
                        debug!("failed to create response: {}", err);
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
            },
            Err(err) => {
                error!("failed to read request: {}", err);
                let _ = stream.write_all("bad request".as_ref()).await;
                return;
            }
        }
    }

    async fn process(
        cmd: String,
        stream: &mut BufStream<UnixStream>,
        zinit: ZInit,
    ) -> Result<Option<Value>> {
        let parts = match shlex::split(&cmd) {
            Some(parts) => parts,
            None => bail!("invalid command syntax"),
        };

        if parts.is_empty() {
            bail!("unknown command");
        }

        if &parts[0] == "log" {
            match parts.len() {
                1 => Self::log(stream, zinit, true).await,
                2 if parts[1] == "snapshot" => Self::log(stream, zinit, false).await,
                _ => bail!("invalid log command arguments"),
            }?;

            return Ok(None);
        }

        let value = match parts[0].as_ref() {
            "list" => Self::list(zinit).await,
            "shutdown" => Self::shutdown(zinit).await,
            "reboot" => Self::reboot(zinit).await,
            "start" if parts.len() == 2 => Self::start(&parts[1], zinit).await,
            "stop" if parts.len() == 2 => Self::stop(&parts[1], zinit).await,
            "kill" if parts.len() == 3 => Self::kill(&parts[1], &parts[2], zinit).await,
            "status" if parts.len() == 2 => Self::status(&parts[1], zinit).await,
            "forget" if parts.len() == 2 => Self::forget(&parts[1], zinit).await,
            "monitor" if parts.len() == 2 => Self::monitor(&parts[1], zinit).await,
            _ => bail!("unknown command '{}' or wrong arguments count", parts[0]),
        }?;

        Ok(Some(value))
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
}

pub struct Client {
    socket: PathBuf,
}

impl Client {
    pub fn new<P: AsRef<Path>>(socket: P) -> Client {
        Client {
            socket: socket.as_ref().to_path_buf(),
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
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::Number(serde_json::Number::from(1))),
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
        let response: JsonRpcResponse = encoder::from_str(&data)?;

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

        let response: Response = encoder::from_str(&data)?;

        match response.state {
            State::Ok => Ok(response.body),
            State::Error => {
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

        self.jsonrpc_request("service.monitor", Some(params)).await?;
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
}