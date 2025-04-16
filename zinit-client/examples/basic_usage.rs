use anyhow::Result;
use serde_json::json;
use std::time::Duration;
use tokio::time;
use zinit_client::Client;

#[tokio::main]
async fn main() -> Result<()> {
    // Create a client using Unix socket transport
    // let unix_client = Client::unix_socket("/var/run/zinit.sock");
    
    // Or create a client using HTTP transport
    let http_client = Client::http("http://localhost:8080");
    
    // Choose which client to use for this example
    let client = http_client;
    
    // Service name for our example
    let service_name = "test1234";
    
    // Step 1: List existing services
    println!("Listing all services before creating our test service:");
    let services = client.list().await?;
    
    // Print all services
    for (name, state) in &services {
        println!("- {}: {}", name, state);
    }
    
    // Step 2: Create a new service
    println!("\n--- Creating service '{}' ---", service_name);
    
    // Create the service configuration
    let service_config = json!({
        "exec": "echo 'hello from test service'",
        "oneshot": false,
        "log": "stdout"
    }).as_object().unwrap().clone();
    
    // Create the service
    let result = client.create_service(service_name, service_config).await?;
    println!("Create result: {}", result);
    
    // Step 3: Monitor the service
    println!("\n--- Monitoring service '{}' ---", service_name);
    client.monitor(service_name).await?;
    println!("Service is now being monitored");
    
    // Wait a moment for the service to start
    println!("Waiting for service to start...");
    time::sleep(Duration::from_secs(2)).await;
    
    // Step 4: Get the status of the service
    println!("\n--- Getting status for '{}' ---", service_name);
    let status = client.status(service_name).await?;
    println!("Name: {}", status.name);
    println!("PID: {}", status.pid);
    println!("State: {}", status.state);
    println!("Target: {}", status.target);
    println!("Dependencies:");
    for (dep, state) in &status.after {
        println!("  - {}: {}", dep, state);
    }
    
    // Step 5: Stop the service
    println!("\n--- Stopping service '{}' ---", service_name);
    client.stop(service_name).await?;
    println!("Service stopped");
    
    // Wait a moment for the service to stop
    time::sleep(Duration::from_secs(1)).await;
    
    // Check status after stopping
    println!("\n--- Status after stopping ---");
    let status = client.status(service_name).await?;
    println!("State: {}", status.state);
    println!("Target: {}", status.target);
    
    // Step 6: Delete the service
    println!("\n--- Deleting service '{}' ---", service_name);
    let delete_result = client.delete_service(service_name).await?;
    println!("Delete result: {}", delete_result);
    
    // Step 7: Verify the service is gone
    println!("\n--- Listing services after deletion ---");
    let services_after = client.list().await?;
    for (name, state) in &services_after {
        println!("- {}: {}", name, state);
    }
    
    // Check if our service was deleted
    if !services_after.contains_key(service_name) {
        println!("\nService '{}' was successfully deleted", service_name);
    } else {
        println!("\nWarning: Service '{}' still exists", service_name);
    }
    
    Ok(())
}