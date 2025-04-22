# Zinit [![Rust](https://github.com/threefoldtech/zinit/actions/workflows/rust.yml/badge.svg)](https://github.com/threefoldtech/zinit/actions/workflows/rust.yml)

Zinit is a lightweight PID 1 replacement inspired by runit, written in Rust using Tokio for async I/O. It provides both a Unix socket interface and an HTTP API for interacting with the process manager.

### Key Features

- **Service Management**: Ensures configured services are up and running at all times
- **Dependency Handling**: Supports service dependencies for proper startup ordering
- **Simple Control Interface**: Provides an intuitive CLI to add, start, stop, and monitor services
- **Container Support**: Can run in container mode with appropriate signal handling
- **Configurable Logging**: Multiple logging options including ringbuffer and stdout

## Installation

Click [here](docs/installation.md) for more information on how to install Zinit.

## Usage

### Process Manager (zinit)

```bash
# Run zinit in init mode
zinit init --config /etc/zinit/ --socket /var/run/zinit.sock

# List services
zinit list

# Start a service
zinit start <service-name>

# Stop a service
zinit stop <service-name>
```

```bash
# Start the HTTP proxy on the default port (8080)
zinit proxy
```

More information about all the available commands can be found [here](docs/cmd.md).

### Service Configuration

Zinit uses YAML files for service configuration. Here's a basic example:

```yaml
# Service configuration (e.g., /etc/zinit/myservice.yaml)
exec: "/usr/bin/myservice --option value"   # Command to run (required)
test: "/usr/bin/check-myservice"            # Health check command (optional)
oneshot: false                              # Whether to restart on exit (default: false)
after:                                      # Services that must be running first (optional)
  - dependency1
  - dependency2
```

For more information on how to configure service files, see the [service file reference](docs/services.md) documentation.

### JSON-RPC API

The HTTP proxy provides a JSON-RPC 2.0 API for interacting with Zinit. You can send JSON-RPC requests to the HTTP endpoint you provided to the proxy:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1,"method":"service_list","params":{}}' http://localhost:8080/
```

See the [OpenRPC specs](openrpc.json) for more information about available RPC calls to interact with Zinit.

## License

See [LICENSE](LICENSE) file for details.
