# Zinit Client Library

A simple Rust client library for interacting with the Zinit process manager.

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

You can create a client using either Unix socket or HTTP transport:

```rust
use zinit_client::Client;

// Using Unix socket (local only)
let client = Client::unix_socket("/var/run/zinit.sock");

// Using HTTP (works for remote Zinit instances)
let client = Client::http("http://localhost:8080");
```

### Service Management

```rust
// List all services
let services = client.list().await?;
for (name, state) in services {
    println!("{}: {}", name, state);
}

// Get status of a specific service
let status = client.status("my-service").await?;
println!("PID: {}, State: {}", status.pid, status.state);

// Start a service
client.start("my-service").await?;

// Stop a service
client.stop("my-service").await?;

// Restart a service
client.restart("my-service").await?;

// Monitor a service
client.monitor("my-service").await?;

// Forget a service
client.forget("my-service").await?;

// Send a signal to a service
client.kill("my-service", "SIGTERM").await?;
```

### Service Configuration

```rust
use serde_json::json;

// Create a new service
let config = json!({
    "exec": "nginx",
    "oneshot": false,
    "after": ["network"]
}).as_object().unwrap().clone();

client.create_service("nginx", config).await?;

// Get service configuration
let config = client.get_service("nginx").await?;
println!("Config: {:?}", config);

// Delete a service
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

The library provides a `ClientError` enum for handling errors:

```rust
match client.status("non-existent-service").await {
    Ok(status) => println!("Service status: {}", status.state),
    Err(e) => match e {
        ClientError::ServiceNotFound(_) => println!("Service not found"),
        ClientError::ConnectionError(_) => println!("Failed to connect to Zinit"),
        _ => println!("Other error: {}", e),
    },
}
```

## Examples

See the [examples](examples) directory for complete usage examples.

## License

This project is licensed under the MIT License.