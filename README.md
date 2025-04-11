# Zinit

Zinit is a lightweight init system and process manager written in Rust, designed to serve as a PID 1 replacement with inspiration from runit. It focuses on essential service management functionality without the complexity of larger init systems.

## Features

- **Lightweight**: Minimal resource footprint, perfect for containers and embedded systems
- **Service Management**: Monitors and maintains service states
- **Dependency Handling**: Manages proper service startup ordering
- **JSON-RPC API**: Modern, structured interface for programmatic control
- **Multiple Clients**: Command-line, library, and HTTP access options
- **Flexible Logging**: Multiple logging options including ringbuffer and stdout

## Quick Start

### Local Installation

```bash
# Build from source
make

# Install the binaries
sudo cp target/x86_64-unknown-linux-musl/zinit /usr/local/bin/
sudo cp target/x86_64-unknown-linux-musl/zinit-http /usr/local/bin/  # Optional HTTP proxy
```

### Using Docker

```bash
# Build the test docker image
make docker

# Run the container with Zinit as init
docker run -dt --device=/dev/kmsg:/dev/kmsg:rw zinit
```
> Don't forget to `-p <XXXX>:<YYYY>` flag when running with Docker if you want to forward the HTTP proxy to your host.

## Basic Usage

### Service Configuration

Create service files in `/etc/zinit/` (default config directory):

```yaml
# /etc/zinit/my-service.yaml
exec: "/usr/bin/my-service --option value"
oneshot: false
after:
  - dependency-service
```

### Command Line

```bash
# List all services
zinit list

# Monitor service
zinit monitor my-service

# Check service status
zinit status my-service

# Start/stop services
zinit start my-service
zinit stop my-service
```

### Using JSON-RPC

```bash
# Example: listing services using the HTTP-proxy
curl -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"service.list","params":[]}' \
  http://localhost:8080/
```

## Documentation

For more detailed documentation, see:

- [Quick Start Guide](docs/quickstart.md) - Get up and running quickly
- [API Documentation](docs/api.md) - JSON-RPC API reference
- [JSON-RPC Usage](docs/json-rpc-usage.md) - Detailed guide with examples
- [Configuration](docs/configuration.md) - Service configuration format
- [HTTP Proxy](docs/zinit-http.md) - Using the zinit-http component
- [Client Library](zinit-client/README.md) - Using the Rust client library
- [Configuration](docs/configuration.md) - Service configuration format

## License

This project is licensed under the Apache 2.0 License - see the [LICENSE](LICENSE) file for details.
