# ZOS Init [![Rust](https://github.com/threefoldtech/zinit/actions/workflows/rust.yml/badge.svg)](https://github.com/threefoldtech/zinit/actions/workflows/rust.yml)

ZOS Init is a lightweight PID 1 replacement inspired by runit, written in Rust using Tokio for async I/O. It manages service startup, supervision, and lifecycle, ensuring configured services remain running and handling dependencies through a simple declarative interface.

## What this is

ZOS Init is an init system and process supervisor designed for environments that need reliable service management without the complexity of traditional init systems. It runs as PID 1 or in container mode, monitors configured services, restarts them on failure, and respects dependency ordering during startup and shutdown. ZOS Init exposes both a Unix socket control interface and an HTTP proxy with a JSON-RPC 2.0 API.

## What this repository contains

- **`zinit` binary** — the init system and process supervisor
- **Unix socket control interface** — for local CLI interaction
- **HTTP proxy** — JSON-RPC 2.0 API for remote management
- **CLI commands** — `init`, `list`, `start`, `stop`, `monitor`, `proxy`, and more
- **Declarative service configuration** via YAML files
- **Container mode** with appropriate signal handling
- **Configurable logging** including ringbuffer and stdout options

## Role in the stack

ZOS Init serves as the init system for ZOS / Zero-OS nodes and can also be used as a standalone process manager in containers or lightweight Linux systems. It is the layer that ensures system services (networking, storage, provisioning, and user workloads) are started in the correct order and kept healthy. External tools and clients — including the ZOS Init Client Rust library — can interact with it over its socket or HTTP APIs.

## ZOS / Zero-OS

ZOS, also known as Zero-OS, is the operating system layer used to run and manage nodes. It provides the low-level runtime environment for workloads, networking, storage, and automation. ZOS Init is the init system at the core of ZOS, responsible for bootstrapping and supervising all node services.

## Relation to ThreeFold

This technology is used within the ThreeFold ecosystem and was first deployed on the ThreeFold Grid. The component itself is designed as reusable infrastructure technology and should be understood by its technical function first, independent of any specific deployment.

## Ownership

This repository is owned and maintained by TF-Tech NV, a Belgian company responsible for the development and maintenance of this technology.

## Installation

```bash
curl https://raw.githubusercontent.com/threefoldtech/zinit/refs/heads/master/install.sh | bash

# to install & run
curl https://raw.githubusercontent.com/threefoldtech/zinit/refs/heads/master/install_run.sh | bash
```

Click [here](docs/installation.md) for more information on how to install ZOS Init.

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

ZOS Init uses YAML files for service configuration. Here's a basic example:

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

The HTTP proxy provides a JSON-RPC 2.0 API for interacting with ZOS Init. You can send JSON-RPC requests to the HTTP endpoint you provided to the proxy:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1,"method":"service_list","params":{}}' http://localhost:8080/
```

See the [OpenRPC specs](openrpc.json) for more information about available RPC calls to interact with ZOS Init.

## License

This project is licensed under the Apache License 2.0 - see the [LICENSE](LICENSE) file for details.
