extern crate zinit;

use anyhow::{Context, Result};
use clap::{App, Arg};
use git_version::git_version;
use std::path::Path;
use zinit::app::client::Client;

const GIT_VERSION: &str = git_version!(args = ["--tags", "--always", "--dirty=-modified"]);

#[tokio::main]
async fn main() -> Result<()> {
    let matches = App::new("zinit-http")
        .author("ThreeFold Tech, https://github.com/threefoldtech")
        .version(GIT_VERSION)
        .about("HTTP proxy for Zinit process manager")
        .arg(
            Arg::with_name("socket")
                .value_name("SOCKET")
                .short("s")
                .long("socket")
                .default_value("/var/run/zinit.sock")
                .help("path to zinit unix socket"),
        )
        .arg(
            Arg::with_name("port")
                .value_name("PORT")
                .short("p")
                .long("port")
                .default_value("8080")
                .help("HTTP server port"),
        )
        .arg(
            Arg::with_name("debug")
                .short("d")
                .long("debug")
                .help("run in debug mode"),
        )
        .get_matches();

    let socket = matches.value_of("socket").unwrap();
    let debug = matches.is_present("debug");
    let port_str = matches.value_of("port").unwrap_or("8080");
    let port = match port_str.parse::<u16>() {
        Ok(port) => port,
        Err(_) => {
            eprintln!("Invalid HTTP port: {}, using default 8080", port_str);
            8080
        }
    };

    // Setup logging
    let level = if debug {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    let logger = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "zinit-http: {} ({}) {}",
                record.level(),
                record.target(),
                message
            ))
        })
        .level(level)
        .chain(std::io::stdout());

    let logger = match std::fs::OpenOptions::new().write(true).open("/dev/kmsg") {
        Ok(file) => logger.chain(file),
        Err(_) => logger,
    };

    if let Err(err) = logger.apply() {
        eprintln!("failed to setup logging: {}", err);
    }

    // Start the HTTP proxy server
    log::info!("Starting Zinit HTTP proxy on port {}", port);
    log::info!("Connecting to Zinit socket at {}", socket);

    // Run the HTTP server
    run_http_server(socket, port).await?;

    Ok(())
}

async fn run_http_server(socket: &str, port: u16) -> Result<()> {
    use axum::{
        body::Bytes,
        http::{HeaderMap, StatusCode},
        response::{IntoResponse, Response},
        routing::{options, post},
        Router,
    };
    use std::net::SocketAddr;
    use tokio::net::TcpListener;

    // JSON-RPC structures (copied from api.rs)
    #[derive(Debug, serde::Deserialize, serde::Serialize)]
    struct JsonRpcRequest {
        jsonrpc: String,
        id: Option<serde_json::Value>,
        method: String,
        params: Option<serde_json::Value>,
    }

    #[derive(Debug, serde::Deserialize, serde::Serialize)]
    struct JsonRpcResponse {
        jsonrpc: String,
        id: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<JsonRpcError>,
    }

    #[derive(Debug, serde::Deserialize, serde::Serialize)]
    struct JsonRpcError {
        code: i32,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
    }

    // Include the OpenRPC specification
    const OPENRPC_SPEC: &str = include_str!("../../openrpc.json");

    // Handle JSON-RPC requests
    async fn handle_json_rpc(
        body: Bytes,
        socket_path: String,
    ) -> Result<Response, (StatusCode, String)> {
        // Process the JSON-RPC request
        let request: JsonRpcRequest = serde_json::from_slice(&body).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("Invalid JSON-RPC request: {}", e),
            )
        })?;

        // We'll manually create a JsonRpcResponse
        let id = request.id.unwrap_or(serde_json::Value::Null);
        let method = request.method.clone();
        let params = request.params.clone();

        // Serialize the request parameters for the Unix socket
        let response_bytes = match method.as_str() {
            "rpc.discover" => {
                // For discover, we can directly return the OpenRPC spec
                let spec: serde_json::Value = match serde_json::from_str(OPENRPC_SPEC) {
                    Ok(value) => value,
                    Err(err) => {
                        return Err((
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("Failed to parse OpenRPC spec: {}", err),
                        ));
                    }
                };

                let response = JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: Some(spec),
                    error: None,
                };

                serde_json::to_vec(&response).map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to serialize response: {}", e),
                    )
                })?
            }
            _ => {
                // Create a client to connect to the Zinit socket
                let client = Client::new(socket_path);

                // Forward the request to Zinit
                let result = match method.as_str() {
                    "service.list" => {
                        let result = client.list().await.map_err(|e| {
                            (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                format!("Failed to execute list: {}", e),
                            )
                        })?;
                        serde_json::to_value(result).map_err(|e| {
                            (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                format!("Failed to serialize result: {}", e),
                            )
                        })?
                    }
                    "service.status" => {
                        if let Some(name) = params
                            .as_ref()
                            .and_then(|p| p.get("name"))
                            .and_then(|v| v.as_str())
                        {
                            let result = client.status(name).await.map_err(|e| {
                                (
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    format!("Failed to execute status: {}", e),
                                )
                            })?;
                            serde_json::to_value(result).map_err(|e| {
                                (
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    format!("Failed to serialize result: {}", e),
                                )
                            })?
                        } else {
                            return Err((
                                StatusCode::BAD_REQUEST,
                                "Missing or invalid 'name' parameter".to_string(),
                            ));
                        }
                    }
                    "service.start" => {
                        if let Some(name) = params
                            .as_ref()
                            .and_then(|p| p.get("name"))
                            .and_then(|v| v.as_str())
                        {
                            client.start(name).await.map_err(|e| {
                                (
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    format!("Failed to execute start: {}", e),
                                )
                            })?;
                            serde_json::Value::Null
                        } else {
                            return Err((
                                StatusCode::BAD_REQUEST,
                                "Missing or invalid 'name' parameter".to_string(),
                            ));
                        }
                    }
                    "service.stop" => {
                        if let Some(name) = params
                            .as_ref()
                            .and_then(|p| p.get("name"))
                            .and_then(|v| v.as_str())
                        {
                            client.stop(name).await.map_err(|e| {
                                (
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    format!("Failed to execute stop: {}", e),
                                )
                            })?;
                            serde_json::Value::Null
                        } else {
                            return Err((
                                StatusCode::BAD_REQUEST,
                                "Missing or invalid 'name' parameter".to_string(),
                            ));
                        }
                    }
                    "service.monitor" => {
                        if let Some(name) = params
                            .as_ref()
                            .and_then(|p| p.get("name"))
                            .and_then(|v| v.as_str())
                        {
                            // For monitoring, we need to check if the service file exists first
                            let file_path = format!("{}.yaml", name);
                            if !std::path::Path::new(&file_path).exists() {
                                return Err((
                                    StatusCode::BAD_REQUEST,
                                    format!("Service file '{}' not found", file_path),
                                ));
                            }

                            // Now we can call monitor
                            client.monitor(name).await.map_err(|e| {
                                (
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    format!("Failed to execute monitor: {}", e),
                                )
                            })?;
                            serde_json::Value::Null
                        } else {
                            return Err((
                                StatusCode::BAD_REQUEST,
                                "Missing or invalid 'name' parameter".to_string(),
                            ));
                        }
                    }
                    "service.forget" => {
                        if let Some(name) = params
                            .as_ref()
                            .and_then(|p| p.get("name"))
                            .and_then(|v| v.as_str())
                        {
                            client.forget(name).await.map_err(|e| {
                                (
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    format!("Failed to execute forget: {}", e),
                                )
                            })?;
                            serde_json::Value::Null
                        } else {
                            return Err((
                                StatusCode::BAD_REQUEST,
                                "Missing or invalid 'name' parameter".to_string(),
                            ));
                        }
                    }
                    "service.kill" => {
                        if let Some(name) = params
                            .as_ref()
                            .and_then(|p| p.get("name"))
                            .and_then(|v| v.as_str())
                        {
                            if let Some(signal) = params
                                .as_ref()
                                .and_then(|p| p.get("signal"))
                                .and_then(|v| v.as_str())
                            {
                                client.kill(name, signal).await.map_err(|e| {
                                    (
                                        StatusCode::INTERNAL_SERVER_ERROR,
                                        format!("Failed to execute kill: {}", e),
                                    )
                                })?;
                                serde_json::Value::Null
                            } else {
                                return Err((
                                    StatusCode::BAD_REQUEST,
                                    "Missing or invalid 'signal' parameter".to_string(),
                                ));
                            }
                        } else {
                            return Err((
                                StatusCode::BAD_REQUEST,
                                "Missing or invalid 'name' parameter".to_string(),
                            ));
                        }
                    }
                    "system.shutdown" => {
                        client.shutdown().await.map_err(|e| {
                            (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                format!("Failed to execute shutdown: {}", e),
                            )
                        })?;
                        serde_json::Value::Null
                    }
                    "system.reboot" => {
                        client.reboot().await.map_err(|e| {
                            (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                format!("Failed to execute reboot: {}", e),
                            )
                        })?;
                        serde_json::Value::Null
                    }
                    "service.create" => {
                        if let Some(name) = params
                            .as_ref()
                            .and_then(|p| p.get("name"))
                            .and_then(|v| v.as_str())
                        {
                            if let Some(content) = params
                                .as_ref()
                                .and_then(|p| p.get("content"))
                                .and_then(|v| v.as_object())
                            {
                                let result = client
                                    .create_service(name, content.clone())
                                    .await
                                    .map_err(|e| {
                                        (
                                            StatusCode::INTERNAL_SERVER_ERROR,
                                            format!("Failed to execute create_service: {}", e),
                                        )
                                    })?;
                                serde_json::to_value(result).map_err(|e| {
                                    (
                                        StatusCode::INTERNAL_SERVER_ERROR,
                                        format!("Failed to serialize result: {}", e),
                                    )
                                })?
                            } else {
                                return Err((
                                    StatusCode::BAD_REQUEST,
                                    "Missing or invalid 'content' parameter".to_string(),
                                ));
                            }
                        } else {
                            return Err((
                                StatusCode::BAD_REQUEST,
                                "Missing or invalid 'name' parameter".to_string(),
                            ));
                        }
                    }
                    "service.delete" => {
                        if let Some(name) = params
                            .as_ref()
                            .and_then(|p| p.get("name"))
                            .and_then(|v| v.as_str())
                        {
                            let result = client.delete_service(name).await.map_err(|e| {
                                (
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    format!("Failed to execute delete_service: {}", e),
                                )
                            })?;
                            serde_json::to_value(result).map_err(|e| {
                                (
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    format!("Failed to serialize result: {}", e),
                                )
                            })?
                        } else {
                            return Err((
                                StatusCode::BAD_REQUEST,
                                "Missing or invalid 'name' parameter".to_string(),
                            ));
                        }
                    }
                    "service.get" => {
                        if let Some(name) = params
                            .as_ref()
                            .and_then(|p| p.get("name"))
                            .and_then(|v| v.as_str())
                        {
                            client.get_service(name).await.map_err(|e| {
                                (
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    format!("Failed to execute get_service: {}", e),
                                )
                            })?
                        } else {
                            return Err((
                                StatusCode::BAD_REQUEST,
                                "Missing or invalid 'name' parameter".to_string(),
                            ));
                        }
                    }
                    // Add other methods as needed
                    _ => {
                        return Err((
                            StatusCode::BAD_REQUEST,
                            format!("Unsupported method: {}", method),
                        ));
                    }
                };

                // Create the JSON-RPC response
                let response = JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: Some(result),
                    error: None,
                };

                serde_json::to_vec(&response).map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to serialize response: {}", e),
                    )
                })?
            }
        };

        // Create response with CORS headers
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", "application/json".parse().unwrap());
        headers.insert("Access-Control-Allow-Origin", "*".parse().unwrap());
        headers.insert(
            "Access-Control-Allow-Methods",
            "POST, OPTIONS".parse().unwrap(),
        );
        headers.insert(
            "Access-Control-Allow-Headers",
            "Content-Type".parse().unwrap(),
        );

        // Return the response
        Ok((headers, Bytes::from(response_bytes)).into_response())
    }

    // Handle OPTIONS requests for CORS
    async fn handle_cors() -> Response {
        // Create response with CORS headers
        let mut headers = HeaderMap::new();
        headers.insert("Access-Control-Allow-Origin", "*".parse().unwrap());
        headers.insert(
            "Access-Control-Allow-Methods",
            "POST, OPTIONS".parse().unwrap(),
        );
        headers.insert(
            "Access-Control-Allow-Headers",
            "Content-Type".parse().unwrap(),
        );

        (headers, "").into_response()
    }

    // Build the router
    let socket_path = socket.to_string();
    let app = Router::new()
        .route(
            "/",
            post(move |body: Bytes| {
                let socket_clone = socket_path.clone();
                handle_json_rpc(body, socket_clone)
            }),
        )
        .route("/", options(handle_cors));

    // Run the server
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    log::info!("Zinit HTTP proxy listening on http://{}", addr);

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
