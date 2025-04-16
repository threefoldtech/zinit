# Zinit Quickstart Guide

This guide will help you get started with Zinit quickly, covering installation, basic setup, and common operations.

## Installation

### From Pre-built Binary (if available)

If you have a pre-built binary for your platform:

```bash
# Copy zinit to a location in your PATH
sudo cp zinit /usr/local/bin/

# Make it executable
sudo chmod +x /usr/local/bin/zinit
```

### Building from Source

To build Zinit from source:

```bash
# Clone the repository
git clone https://github.com/threefoldtech/zinit.git
cd zinit

# Build using make (requires Rust, musl and musl-tools)
make

# Install the binary
sudo cp target/x86_64-unknown-linux-musl/release/zinit /usr/local/bin/
```

## Quick Test with Docker

For a quick test without installing:

```bash
# Build the test Docker image
make docker

# Run the container
docker run -dt --device=/dev/kmsg:/dev/kmsg:rw zinit
```

## Basic Configuration

### Directory Structure

Zinit looks for service configuration files in `/etc/zinit/` by default. Create this directory if it doesn't exist:

```bash
sudo mkdir -p /etc/zinit
```

### Create a Simple Service

Let's create a simple service that pings Google's DNS server:

```bash
cat << EOF | sudo tee /etc/zinit/ping.yaml
exec: "ping 8.8.8.8"
log: stdout
EOF
```

This service will:
- Execute the `ping 8.8.8.8` command
- Log output to Zinit's stdout

## Running Zinit

### As Init System (PID 1)

To run Zinit as the init system (typically in production or in a container):

```bash
# Run as PID 1
sudo zinit init
```

### In Container Mode

When running in a container:

```bash
sudo zinit init --container
```

### With a Custom Configuration Directory

```bash
sudo zinit init -c /path/to/config/dir
```

## Managing Services

### List Services

To see all configured services:

```bash
zinit list
```

Example output:
```json
{"state":"ok","body":{"ping":"Running"}}
```

### Check Service Status

To check the status of a specific service:

```bash
zinit status ping
```

Example output:
```json
{"state":"ok","body":{"name":"ping","pid":1234,"state":"Running","target":"Up"}}
```

### Start, Stop, and Restart

```bash
# Stop a service
zinit stop ping

# Start a service
zinit start ping

# Restart a service
zinit restart ping
```

### Adding a New Service at Runtime

Create a new service configuration file:

```bash
cat << EOF | sudo tee /etc/zinit/hello.yaml
exec: "echo 'Hello, World!' && sleep 10"
oneshot: true
EOF
```

Then tell Zinit to monitor it:

```bash
zinit monitor hello
```

### View Service Logs

```bash
# View all logs
zinit log

# View logs for a specific service
zinit log hello

# View current logs without following
zinit log -s
```

## Creating Complex Services

### Service with Dependencies

```bash
# Create a database service
cat << EOF | sudo tee /etc/zinit/database.yaml
exec: "echo 'Starting database' && sleep infinity"
EOF

# Create an application that depends on the database
cat << EOF | sudo tee /etc/zinit/application.yaml
exec: "echo 'Starting application' && sleep infinity"
after:
  - database
EOF
```

When you monitor both services, Zinit will ensure the database starts before the application.

### One-shot Initialization Service

For a service that runs once and doesn't restart:

```bash
cat << EOF | sudo tee /etc/zinit/init-data.yaml
exec: "echo 'Initializing data...'; sleep 2; echo 'Done!'"
oneshot: true
EOF
```

### Service with a Test Command

A service that is only considered running when the test passes:

```bash
cat << EOF | sudo tee /etc/zinit/webserver.yaml
exec: "python3 -m http.server 8080"
test: "curl -s http://localhost:8080 > /dev/null"
EOF
```

### Service with Custom Stop Signal

By default, Zinit uses SIGTERM to stop services. You can customize this:

```bash
cat << EOF | sudo tee /etc/zinit/graceful-app.yaml
exec: "/usr/local/bin/my-app"
signal:
  stop: SIGINT  # Use SIGINT for graceful shutdown
EOF
```

## System Control

### Shutdown and Reboot

To safely stop all services and shut down the system:

```bash
zinit shutdown
```

To reboot the system:

```bash
zinit reboot
```

## Troubleshooting

### Service won't start

Check for errors in the service configuration:

```bash
# Check service status
zinit status myservice

# Check logs
zinit log myservice
```

### Finding a service's exit code

If a service keeps failing, check its status to see the exit code:

```bash
zinit status myservice
```

Look for the `state` field in the output, which may show something like `Error(Exited(1))`.

### Force stopping a stuck service

If a service doesn't respond to the normal stop command:

```bash
# Send SIGKILL
zinit kill myservice SIGKILL
```

## Next Steps

- Read the [detailed service configuration guide](../configuration/service-format.md)
- Learn about [service lifecycle management](../architecture/lifecycle.md) 
- Explore the [complete command reference](../usage/commands.md)