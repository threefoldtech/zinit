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
