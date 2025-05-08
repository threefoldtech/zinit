use anyhow::Result;
use serde_json::json;
use zinit_client::Client;

#[tokio::main]
async fn main() -> Result<()> {
    // Create a client using HTTP transport
    let client = Client::http("http://localhost:8080").await?;

    // Create a new service
    let service_name = "example-http-service";
    let service_config = json!({
        "exec": "echo 'Hello from HTTP service'",
        "oneshot": true,
        "after": ["network"]
    })
    .as_object()
    .unwrap()
    .clone();

    match client.create_service(service_name, service_config).await {
        Ok(msg) => println!("Service created: {}", msg),
        Err(e) => eprintln!("Failed to create service: {}", e),
    }

    // Start the HTTP/RPC server on a specific address
    match client.start_http_server("0.0.0.0:8081").await {
        Ok(msg) => println!("HTTP server status: {}", msg),
        Err(e) => eprintln!("Failed to start HTTP server: {}", e),
    }

    // List all services
    let services = client.list().await?;
    println!("\nServices:");
    for (name, state) in services {
        println!("{}: {}", name, state);
    }

    // Monitor the service
    match client.monitor(service_name).await {
        Ok(_) => println!("\nService is now monitored"),
        Err(e) => eprintln!("Failed to monitor service: {}", e),
    }

    // Start the service
    match client.start(service_name).await {
        Ok(_) => println!("Service started successfully"),
        Err(e) => eprintln!("Failed to start service: {}", e),
    }

    // Get logs
    let logs = client.logs(Some(service_name.to_string())).await?;
    println!("\nLogs:");
    for log in logs {
        println!("{}", log);
    }

    // Clean up - forget the service
    println!("\nCleaning up...");
    match client.forget(service_name).await {
        Ok(_) => println!("Service has been forgotten"),
        Err(e) => eprintln!("Failed to forget service: {}", e),
    }

    // Clean up - delete the service configuration
    match client.delete_service(service_name).await {
        Ok(msg) => println!("{}", msg),
        Err(e) => eprintln!("Failed to delete service: {}", e),
    }

    // Stop the HTTP/RPC server
    match client.stop_http_server().await {
        Ok(_) => println!("HTTP server stopped"),
        Err(e) => eprintln!("Failed to stop HTTP server: {}", e),
    }

    Ok(())
}
