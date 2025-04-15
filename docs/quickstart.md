# Zinit Quick Start Guide

This guide will help you get up and running with Zinit quickly, covering installation, basic configuration, and common operations.

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/threefoldtech/zinit.git
cd zinit

# Build the binaries
cargo build --release

# Install binaries
sudo cp target/release/zinit /usr/local/bin/
sudo cp target/release/zinit-http /usr/local/bin/  # Optional HTTP proxy
```

### Using Docker

```bash
# Build the test docker image
make docker

# Run the container with Zinit as PID 1
docker run -dt --device=/dev/kmsg:/dev/kmsg:rw zinit
```

## Service Configuration

Create a service configuration directory:

```bash
sudo mkdir -p /etc/zinit
```

Create a simple service file:

```bash
cat > /etc/zinit/hello.yaml << EOF
exec: "/bin/sh -c 'while true; do echo Hello World; sleep 5; done'"
EOF
```

## Starting Zinit

### As a Regular Process

```bash
# Start Zinit in the foreground
zinit init --config /etc/zinit/

# Or start in the background
zinit init --config /etc/zinit/ &
```

### As PID 1 (Init System)

Zinit can be used as the primary init system (PID 1) in a container or embedded system:

```bash
# Docker example
docker run -it --init=false -v /path/to/configs:/etc/zinit threefoldtech/zinit
```

## Basic Operations

### Viewing Services

```bash
# List all services and their states
zinit list
```

### Starting and Stopping Services

```bash
# Start a service
zinit start hello

# Check service status
zinit status hello

# Stop a service
zinit stop hello
```

### Managing Service Configuration

```bash
# Add a new service (from file)
cat > /etc/zinit/redis.yaml << EOF
exec: "/usr/bin/redis-server"
after:
  - network
EOF

# Make Zinit aware of the service
zinit monitor redis

# Remove a service (must be stopped first)
zinit stop redis
zinit forget redis
```

## Using the JSON-RPC API

### Direct Unix Socket Access

```bash
# List all services
curl --unix-socket /var/run/zinit.sock -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"service.list","params":[]}' \
  http://localhost/
```

### Using the HTTP Proxy

```bash
# Start the HTTP proxy (if not running)
zinit-http &

# List all services via HTTP
curl -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"service.list","params":[]}' \
  http://localhost:8080/
```

## Using the Client Library

For Rust applications, add the client library to your `Cargo.toml`:

```toml
[dependencies]
zinit-client = "0.1.0"
```

Then use it in your code:

```rust
use zinit_client::Client;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let client = Client::unix_socket("/var/run/zinit.sock");
    
    // List all services
    let services = client.list().await?;
    for (name, state) in services {
        println!("{}: {}", name, state);
    }
    
    Ok(())
}
```

## Next Steps

- Learn about [service configuration options](configuration.md)
- Explore the [JSON-RPC API](api.md) in detail
- See [JSON-RPC usage examples](json-rpc-usage.md)
- Set up the [HTTP proxy](zinit-http.md) for remote access
- Use the [client library](../zinit-client/README.md) for Rust applications