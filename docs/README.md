# Zinit Documentation

Welcome to the Zinit documentation. This guide will help you understand and use Zinit, a lightweight init system written in Rust.

## What is Zinit?

Zinit is a lightweight PID 1 replacement (init system) inspired by runit. It manages services, handles dependencies, and provides a simple control interface.

## Documentation Structure

The documentation is organized into these main sections:

- **Getting Started**: Quick setup and installation
- **Configuration**: Service file format and examples
- **Usage**: Command reference and workflows
- **Architecture**: System design and implementation
- **API**: Protocol for communicating with Zinit

## Key Concepts

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

### Basic Commands

```bash
# Run zinit as init
zinit init

# List all services
zinit list

# Check service status
zinit status myservice

# Start/stop services
zinit start myservice
zinit stop myservice
```

## Getting Started

For a quick introduction, see the [Quickstart Guide](getting-started/quickstart.md).

For more detailed documentation, start with:

1. [Installation Guide](getting-started/installation.md)
2. [Service Configuration Format](configuration/service-format.md)
3. [Command Reference](usage/commands.md)

## Legacy Documentation

The older documentation is still available:
- [Legacy Implementation Details](implementation.md)
- [Legacy Protocol Documentation](protocol.md)