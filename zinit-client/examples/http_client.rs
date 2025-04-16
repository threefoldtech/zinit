use anyhow::Result;
use zinit_client::Client;

#[tokio::main]
async fn main() -> Result<()> {
    // Create a client using HTTP transport
    let client = Client::http("http://localhost:8080");
    
    println!("Connecting to Zinit via HTTP...");
    
    // List all services
    println!("Listing all services:");
    match client.list().await {
        Ok(services) => {
            if services.is_empty() {
                println!("No services found.");
            } else {
                for (name, state) in &services {
                    println!("- {}: {}", name, state);
                }
                
                // Try to get the first service for a status example
                if let Some(service_name) = services.keys().next() {
                    println!("\nGetting status for {}:", service_name);
                    match client.status(service_name).await {
                        Ok(status) => {
                            println!("Name: {}", status.name);
                            println!("PID: {}", status.pid);
                            println!("State: {}", status.state);
                            println!("Target: {}", status.target);
                            println!("Dependencies: {}", status.after.len());
                        },
                        Err(e) => println!("Error getting status: {}", e),
                    }
                }
            }
        },
        Err(e) => {
            println!("Error connecting to Zinit HTTP proxy: {}", e);
            println!("Make sure the Zinit HTTP proxy is running on http://localhost:8080");
        }
    }
    
    Ok(())
}