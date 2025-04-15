# Zinit Client Library

A Rust client library for interacting with the Zinit process manager using the JSON-RPC API.

## Features

- Connect to Zinit via Unix socket or HTTP
- Manage services (start, stop, restart, monitor)
- Query service status and information
- Create and delete service configurations
- System operations (shutdown, reboot)

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
zinit-client = "0.1.0"
```

## Usage

### Creating a Client

The library supports two transport methods:

```rust
use zinit_client::Client;

// Using Unix socket (local only)
let client = Client::unix_socket("/var/run/zinit.sock");

// Using HTTP (works for remote Zinit instances via zinit-http)
let client = Client::http("http://localhost:8080");
```

### Service Management

```rust
// List all services
let services = client.list().await?;
for (name, state) in services {
    println!("{}: {}", name, state);
}

// Get detailed status of a specific service
let status = client.status("redis").await?;
println!("PID: {}, State: {}", status.pid, status.state);

// Start a service
client.start("redis").await?;

// Stop a service
client.stop("redis").await?;

// Monitor a service (load config and start managing it)
client.monitor("redis").await?;

// Forget a service (stop tracking it)
client.forget("redis").await?;

// Send a signal to a service
client.kill("redis", "SIGTERM").await?;
```

### Service Configuration Management

```rust
use serde_json::json;

// Create a new service configuration
let config = json!({
    "exec": "/usr/bin/nginx",
    "oneshot": false,
    "after": ["network"],
    "env": {
        "NGINX_OPTS": "-c /etc/nginx/custom.conf"
    }
}).as_object().unwrap().clone();

client.create_service("nginx", config).await?;

// Get a service's configuration
let config = client.get_service("nginx").await?;
println!("Exec command: {}", config.get("exec").unwrap());

// Delete a service configuration
client.delete_service("nginx").await?;
```

### System Operations

```rust
// Shutdown the system
client.shutdown().await?;

// Reboot the system
client.reboot().await?;
```

## Error Handling

The library provides a `ClientError` enum for handling various error conditions:

```rust
match client.status("non-existent-service").await {
    Ok(status) => println!("Service status: {}", status.state),
    Err(e) => match e {
        ClientError::ServiceNotFound(_) => println!("Service not found"),
        ClientError::ConnectionError(_) => println!("Failed to connect to Zinit"),
        ClientError::ServiceIsUp(_) => println!("Service is already running"),
        ClientError::ServiceIsDown(_) => println!("Service is not running"),
        _ => println!("Other error: {}", e),
    },
}
```

## Complete Example

```rust
use zinit_client::Client;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // Create a client using Unix socket
    let client = Client::unix_socket("/var/run/zinit.sock");
    
    // List all services and their states
    let services = client.list().await?;
    println!("Services:");
    for (name, state) in services {
        println!("  {}: {}", name, state);
    }
    
    // Try to start a service
    match client.start("my-service").await {
        Ok(_) => println!("Service started successfully"),
        Err(e) => println!("Failed to start service: {}", e),
    }
    
    Ok(())
}
```

## HTTP Client Example

```rust
use zinit_client::Client;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // Create a client using HTTP (requires zinit-http to be running)
    let client = Client::http("http://localhost:8080");
    
    // Get status of a specific service
    let status = client.status("redis").await?;
    println!("Redis service: {}", status.state);
    
    Ok(())
}
```

For more examples, see the [examples](examples) directory.

## License

This project is licensed under the Apache 2.0 License.