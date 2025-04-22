#[tokio::main]
async fn main () {
    println!("hello from testapp");
}


// extern crate zinit;

// use anyhow::Result;
// use serde_json::json;
// use std::env;
// use tokio::time::{sleep, Duration};

// use zinit::testapp;

// #[tokio::main]
// async fn main() -> Result<()> {
//     // Define paths for socket and config
//     let temp_dir = env::temp_dir();
//     let socket_path = temp_dir
//         .join("zinit-test.sock")
//         .to_str()
//         .unwrap()
//         .to_string();
//     let config_dir = temp_dir
//         .join("zinit-test-config")
//         .to_str()
//         .unwrap()
//         .to_string();

//     println!("Starting zinit with socket at: {}", socket_path);
//     println!("Using config directory: {}", config_dir);

//     // Start zinit in the background
//     testapp::start_zinit(&socket_path, &config_dir).await?;

//     // Wait for zinit to initialize
//     sleep(Duration::from_secs(2)).await;

//     // Create a client to communicate with zinit
//     let client = Client::new(&socket_path);

//     // Create service configurations
//     println!("Creating service configurations...");

//     // Create a find service
//     testapp::create_service_config(
//         &config_dir,
//         "find-service",
//         "find / -name \"*.txt\" -type f",
//     )
//     .await?;

//     // Create a sleep service with echo
//     testapp::create_service_config(
//         &config_dir,
//         "sleep-service",
//         "sh -c 'echo Starting sleep; sleep 30; echo Finished sleep'",
//     )
//     .await?;

//     // Wait for zinit to load the configurations
//     sleep(Duration::from_secs(1)).await;

//     // Tell zinit to monitor our services
//     println!("Monitoring services...");
//     client.monitor("find-service").await?;
//     client.monitor("sleep-service").await?;

//     // List all services
//     println!("\nListing all services:");
//     let services = client.list().await?;
//     for (name, status) in services {
//         println!("Service: {} - Status: {}", name, status);
//     }

//     // Start the find service
//     println!("\nStarting find-service...");
//     client.start("find-service").await?;

//     // Wait a bit and check status
//     sleep(Duration::from_secs(2)).await;
//     let status = client.status("find-service").await?;
//     println!("find-service status: {:?}", status);

//     // Start the sleep service
//     println!("\nStarting sleep-service...");
//     client.start("sleep-service").await?;

//     // Wait a bit and check status
//     sleep(Duration::from_secs(2)).await;
//     let status = client.status("sleep-service").await?;
//     println!("sleep-service status: {:?}", status);

//     // Stop the find service
//     println!("\nStopping find-service...");
//     client.stop("find-service").await?;

//     // Wait a bit and check status
//     sleep(Duration::from_secs(2)).await;
//     let status = client.status("find-service").await?;
//     println!("find-service status after stopping: {:?}", status);

//     // Kill the sleep service with SIGTERM
//     println!("\nKilling sleep-service with SIGTERM...");
//     client.kill("sleep-service", "SIGTERM").await?;

//     // Wait a bit and check status
//     sleep(Duration::from_secs(2)).await;
//     let status = client.status("sleep-service").await?;
//     println!("sleep-service status after killing: {:?}", status);

//     // Cleanup - forget services
//     println!("\nForgetting services...");
//     if status.pid == 0 {
//         // Only forget if it's not running
//         client.forget("sleep-service").await?;
//     }
//     client.forget("find-service").await?;

//     // Demonstrate service file operations
//     println!("\nDemonstrating service file operations...");

//     // Create a new service using the API
//     println!("Creating a new service via API...");
//     let service_content = json!({
//         "exec": "echo 'Hello from API-created service'",
//         "oneshot": true,
//         "log": "stdout"
//     })
//     .as_object()
//     .unwrap()
//     .clone();

//     let result = client
//         .create_service("api-service", service_content)
//         .await?;
//     println!("Create service result: {}", result);

//     // Get the service configuration
//     println!("\nGetting service configuration...");
//     let config = client.get_service("api-service").await?;
//     println!(
//         "Service configuration: {}",
//         serde_json::to_string_pretty(&config)?
//     );

//     // Monitor and start the new service
//     println!("\nMonitoring and starting the new service...");
//     client.monitor("api-service").await?;
//     client.start("api-service").await?;

//     // Wait a bit and check status
//     sleep(Duration::from_secs(2)).await;
//     let status = client.status("api-service").await?;
//     println!("api-service status: {:?}", status);

//     // Delete the service
//     println!("\nDeleting the service...");
//     if status.pid == 0 {
//         // Only forget if it's not running
//         client.forget("api-service").await?;
//         let result = client.delete_service("api-service").await?;
//         println!("Delete service result: {}", result);
//     }

//     // Shutdown zinit
//     println!("\nShutting down zinit...");
//     client.shutdown().await?;

//     println!("\nTest completed successfully!");
//     Ok(())
// }
