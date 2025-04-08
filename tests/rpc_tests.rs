use anyhow::Result;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Once;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::time::sleep;

// Initialize test environment once
static INIT: Once = Once::new();

// Helper function to initialize the test environment
fn initialize() {
    INIT.call_once(|| {
        // Create a temporary directory for tests
        let _ = std::fs::create_dir_all("/tmp/zinit-test");
        
        // Create a test service configuration
        let _ = std::fs::write(
            "/tmp/zinit-test/test-service.yaml",
            r#"
exec: echo "Test service running"
oneshot: true
            "#,
        );
    });
}

// Helper function to send a JSON-RPC request and get the response
async fn send_jsonrpc_request(method: &str, params: Option<Value>) -> Result<Value> {
    // Connect to the Unix socket
    let socket_path = "/tmp/zinit-test/zinit.sock";
    let mut stream = UnixStream::connect(socket_path).await?;
    
    // Create the JSON-RPC request
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params
    });
    
    // Send the request
    let request_bytes = serde_json::to_vec(&request)?;
    stream.write_all(&request_bytes).await?;
    stream.flush().await?;
    
    // Read the response
    let mut buffer = Vec::new();
    stream.read_to_end(&mut buffer).await?;
    
    // Parse the response
    let response: serde_json::Value = serde_json::from_slice(&buffer)?;
    
    // Check if there's an error
    if let Some(error) = response.get("error") {
        anyhow::bail!("RPC error: {}", error);
    }
    
    // Return the result
    if let Some(result) = response.get("result") {
        Ok(result.clone())
    } else {
        anyhow::bail!("Invalid JSON-RPC response: missing result")
    }
}

// Helper function to send a batch JSON-RPC request and get the responses
async fn send_jsonrpc_batch_request(requests: Vec<(String, Option<Value>)>) -> Result<Vec<Value>> {
    // Connect to the Unix socket
    let socket_path = "/tmp/zinit-test/zinit.sock";
    let mut stream = UnixStream::connect(socket_path).await?;
    
    // Create the JSON-RPC batch request
    let mut batch = Vec::new();
    for (i, (method, params)) in requests.iter().enumerate() {
        batch.push(json!({
            "jsonrpc": "2.0",
            "id": i + 1,
            "method": method,
            "params": params
        }));
    }
    
    // Send the request
    let request_bytes = serde_json::to_vec(&batch)?;
    stream.write_all(&request_bytes).await?;
    stream.flush().await?;
    
    // Read the response
    let mut buffer = Vec::new();
    stream.read_to_end(&mut buffer).await?;
    
    // Parse the response
    let responses: Vec<serde_json::Value> = serde_json::from_slice(&buffer)?;
    
    // Extract the results
    let mut results = Vec::new();
    for response in responses {
        // Check if there's an error
        if let Some(error) = response.get("error") {
            anyhow::bail!("RPC error: {}", error);
        }
        
        // Add the result
        if let Some(result) = response.get("result") {
            results.push(result.clone());
        } else {
            anyhow::bail!("Invalid JSON-RPC response: missing result")
        }
    }
    
    Ok(results)
}

// Helper function to start a test Zinit instance
fn start_test_zinit() -> Result<std::process::Child> {
    // Start Zinit in a separate process
    let child = Command::new("cargo")
        .args(&[
            "run",
            "--",
            "init",
            "--container",
            "-c", "/tmp/zinit-test",
            "-s", "/tmp/zinit-test/zinit.sock",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    
    // Wait for Zinit to start
    std::thread::sleep(Duration::from_secs(1));
    
    Ok(child)
}

// Helper function to stop a test Zinit instance
fn stop_test_zinit(mut child: std::process::Child) -> Result<()> {
    // Kill the process
    child.kill()?;
    child.wait()?;
    
    Ok(())
}

// Test the service.list method
#[tokio::test]
async fn test_list() -> Result<()> {
    initialize();
    
    // Start a test Zinit instance
    let child = start_test_zinit()?;
    
    // Wait for Zinit to start
    sleep(Duration::from_secs(1)).await;
    
    // Send a JSON-RPC request to list services
    let result = send_jsonrpc_request("service.list", None).await?;
    
    // Check the result
    let services: HashMap<String, String> = serde_json::from_value(result)?;
    assert!(services.is_empty() || services.contains_key("test-service"));
    
    // Stop the test Zinit instance
    stop_test_zinit(child)?;
    
    Ok(())
}

// Test the service.monitor and service.status methods
#[tokio::test]
async fn test_monitor_and_status() -> Result<()> {
    initialize();
    
    // Start a test Zinit instance
    let child = start_test_zinit()?;
    
    // Wait for Zinit to start
    sleep(Duration::from_secs(1)).await;
    
    // Send a JSON-RPC request to monitor a service
    let params = json!({
        "name": "test-service"
    });
    let _ = send_jsonrpc_request("service.monitor", Some(params.clone())).await?;
    
    // Wait for the service to start
    sleep(Duration::from_secs(1)).await;
    
    // Send a JSON-RPC request to get the service status
    let result = send_jsonrpc_request("service.status", Some(params)).await?;
    
    // Check the result
    let status: serde_json::Value = result;
    assert_eq!(status["name"], "test-service");
    
    // Stop the test Zinit instance
    stop_test_zinit(child)?;
    
    Ok(())
}

// Test the service.start and service.stop methods
#[tokio::test]
async fn test_start_and_stop() -> Result<()> {
    initialize();
    
    // Start a test Zinit instance
    let child = start_test_zinit()?;
    
    // Wait for Zinit to start
    sleep(Duration::from_secs(1)).await;
    
    // Send a JSON-RPC request to monitor a service
    let params = json!({
        "name": "test-service"
    });
    let _ = send_jsonrpc_request("service.monitor", Some(params.clone())).await?;
    
    // Wait for the service to start
    sleep(Duration::from_secs(1)).await;
    
    // Send a JSON-RPC request to stop the service
    let _ = send_jsonrpc_request("service.stop", Some(params.clone())).await?;
    
    // Wait for the service to stop
    sleep(Duration::from_secs(1)).await;
    
    // Send a JSON-RPC request to start the service
    let _ = send_jsonrpc_request("service.start", Some(params.clone())).await?;
    
    // Wait for the service to start
    sleep(Duration::from_secs(1)).await;
    
    // Send a JSON-RPC request to get the service status
    let result = send_jsonrpc_request("service.status", Some(params)).await?;
    
    // Check the result
    let status: serde_json::Value = result;
    assert_eq!(status["name"], "test-service");
    
    // Stop the test Zinit instance
    stop_test_zinit(child)?;
    
    Ok(())
}

// Test batch requests
#[tokio::test]
async fn test_batch_request() -> Result<()> {
    initialize();
    
    // Start a test Zinit instance
    let child = start_test_zinit()?;
    
    // Wait for Zinit to start
    sleep(Duration::from_secs(1)).await;
    
    // Send a JSON-RPC request to monitor a service
    let params = json!({
        "name": "test-service"
    });
    let _ = send_jsonrpc_request("service.monitor", Some(params.clone())).await?;
    
    // Wait for the service to start
    sleep(Duration::from_secs(1)).await;
    
    // Create a batch request
    let requests = vec![
        ("service.list".to_string(), None),
        ("service.status".to_string(), Some(params)),
    ];
    
    // Send the batch request
    let results = send_jsonrpc_batch_request(requests).await?;
    
    // Check the results
    assert_eq!(results.len(), 2);
    
    // Check the first result (service.list)
    let services: HashMap<String, String> = serde_json::from_value(results[0].clone())?;
    assert!(services.contains_key("test-service"));
    
    // Check the second result (service.status)
    let status: serde_json::Value = results[1].clone();
    assert_eq!(status["name"], "test-service");
    
    // Stop the test Zinit instance
    stop_test_zinit(child)?;
    
    Ok(())
}

// Test error handling
#[tokio::test]
async fn test_error_handling() -> Result<()> {
    initialize();
    
    // Start a test Zinit instance
    let child = start_test_zinit()?;
    
    // Wait for Zinit to start
    sleep(Duration::from_secs(1)).await;
    
    // Send a JSON-RPC request with an invalid method
    let socket_path = "/tmp/zinit-test/zinit.sock";
    let mut stream = UnixStream::connect(socket_path).await?;
    
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "invalid_method",
        "params": null
    });
    
    let request_bytes = serde_json::to_vec(&request)?;
    stream.write_all(&request_bytes).await?;
    stream.flush().await?;
    
    let mut buffer = Vec::new();
    stream.read_to_end(&mut buffer).await?;
    
    let response: serde_json::Value = serde_json::from_slice(&buffer)?;
    
    // Check that there's an error
    assert!(response.get("error").is_some());
    assert_eq!(response["error"]["code"], METHOD_NOT_FOUND);
    
    // Send a JSON-RPC request with invalid parameters
    let mut stream = UnixStream::connect(socket_path).await?;
    
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "service.status",
        "params": null
    });
    
    let request_bytes = serde_json::to_vec(&request)?;
    stream.write_all(&request_bytes).await?;
    stream.flush().await?;
    
    let mut buffer = Vec::new();
    stream.read_to_end(&mut buffer).await?;
    
    let response: serde_json::Value = serde_json::from_slice(&buffer)?;
    
    // Check that there's an error
    assert!(response.get("error").is_some());
    assert_eq!(response["error"]["code"], INVALID_PARAMS);
    
    // Stop the test Zinit instance
    stop_test_zinit(child)?;
    
    Ok(())
}

// Test the original line-based protocol (backward compatibility)
#[tokio::test]
async fn test_line_protocol() -> Result<()> {
    initialize();
    
    // Start a test Zinit instance
    let child = start_test_zinit()?;
    
    // Wait for Zinit to start
    sleep(Duration::from_secs(1)).await;
    
    // Connect to the Unix socket
    let socket_path = "/tmp/zinit-test/zinit.sock";
    let mut stream = UnixStream::connect(socket_path).await?;
    
    // Send a line-based command
    stream.write_all(b"list\n").await?;
    stream.flush().await?;
    
    // Read the response
    let mut buffer = Vec::new();
    stream.read_to_end(&mut buffer).await?;
    
    // Parse the response
    let response: serde_json::Value = serde_json::from_slice(&buffer)?;
    
    // Check the response
    assert!(response.get("state").is_some());
    assert_eq!(response["state"], "ok");
    
    // Stop the test Zinit instance
    stop_test_zinit(child)?;
    
    Ok(())
}

// Constants for error codes (must match those in src/app/api.rs)
const METHOD_NOT_FOUND: i32 = -32601;
const INVALID_PARAMS: i32 = -32602;