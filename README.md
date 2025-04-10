# Zinit - A Process Manager

Zinit is a process manager designed to manage services and their lifecycle. It provides both a Unix socket interface and an HTTP API for interacting with the process manager.

## Components

Zinit now consists of two separate binaries:

1. **zinit** - The core process manager that handles service lifecycle management
2. **zinit-http** - An HTTP proxy that forwards JSON-RPC requests to the Zinit Unix socket

This separation allows for more flexibility and reduced resource usage when the HTTP API is not needed.

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

### HTTP Proxy (zinit-http)

```bash
# Start the HTTP proxy on the default port (8080)
zinit-http --socket /var/run/zinit.sock

# Start the HTTP proxy on a custom port
zinit-http --socket /var/run/zinit.sock --port 3000
```

## JSON-RPC API

The HTTP proxy provides a JSON-RPC 2.0 API for interacting with Zinit. You can send JSON-RPC requests to the HTTP endpoint:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1,"method":"service.list","params":{}}' http://localhost:8080/
```

## Building from Source

```bash
# Build both binaries
cargo build --release

# The binaries will be available in target/release/
```

# Zinit [![Rust](https://github.com/threefoldtech/zinit/actions/workflows/rust.yml/badge.svg)](https://github.com/threefoldtech/zinit/actions/workflows/rust.yml)

A lightweight PID 1 replacement inspired by runit, written in Rust using Tokio for async I/O.

## Overview

Zinit is a service manager designed to be simple, lightweight, and reliable for both system services and container environments. It acts as an init system (PID 1) but focuses only on essential service management functionality.

### Key Features

- **Service Management**: Ensures configured services are up and running at all times
- **Dependency Handling**: Supports service dependencies for proper startup ordering
- **Simple Control Interface**: Provides an intuitive CLI to add, start, stop, and monitor services
- **Container Support**: Can run in container mode with appropriate signal handling
- **Configurable Logging**: Multiple logging options including ringbuffer and stdout

## Documentation

Comprehensive documentation is available in the [docs](docs) directory:

- [Command Line Interface](docs/README.md)
- [Implementation Details](docs/implementation.md)
- [API Protocol](docs/protocol.md)

## Quick Start

### Installation

```bash
# Build from source
make

# Install the binary
sudo cp target/x86_64-unknown-linux-musl/release/zinit /usr/local/bin/
```

### Testing with Docker

To quickly try zinit in a container environment:

```bash
# Build the test docker image
make docker

# Run the container
docker run -dt --device=/dev/kmsg:/dev/kmsg:rw zinit
```

The test image automatically starts Redis and OpenSSH services.

## Building from Source

### Requirements

- Rust (cargo) - version 1.46.0 or later
- musl and musl-tools packages
- GNU Make

### Build Instructions

```bash
# Standard build
make

# For development/debug
make dev
```

## License

See [LICENSE](LICENSE) file for details.
