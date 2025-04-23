use anyhow::Result;
use zinit_client::Client;

#[tokio::main]
async fn main() -> Result<()> {
    // Create a client using Unix socket transport
    let client = Client::unix_socket("/var/run/zinit.sock").await?;

    // List all services
    let services = client.list().await?;
    println!("Services:");
    for (name, state) in services {
        println!("{}: {}", name, state);
    }

    // Get a specific service status
    let service_name = "example-service";
    match client.status(service_name).await {
        Ok(status) => {
            println!("\nService: {}", status.name);
            println!("PID: {}", status.pid);
            println!("State: {}", status.state);
            println!("Target: {}", status.target);
            println!("After:");
            for (dep, state) in status.after {
                println!("  {}: {}", dep, state);
            }
        }
        Err(e) => eprintln!("Failed to get status: {}", e),
    }

    // Try to start a service
    match client.start(service_name).await {
        Ok(_) => println!("\nService started successfully"),
        Err(e) => eprintln!("Failed to start service: {}", e),
    }

    // Get logs for the service
    match client.logs(Some(service_name.to_string())).await {
        Ok(logs) => {
            println!("\nLogs:");
            for log in logs {
                println!("{}", log);
            }
        }
        Err(e) => eprintln!("Failed to get logs: {}", e),
    }

    Ok(())
}
