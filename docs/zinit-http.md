# Zinit-HTTP: HTTP Proxy for Zinit

`zinit-http` is a standalone binary that provides HTTP access to Zinit's JSON-RPC API. This component enables remote management of Zinit services and system operations.

## Overview

While the main Zinit process manager communicates via a Unix socket, the `zinit-http` proxy translates HTTP requests into Unix socket communications, allowing:

- Remote access to Zinit from other machines
- Integration with web services and applications
- Access from environments where Unix socket communication isn't possible

## Installation

The `zinit-http` binary is built alongside the main Zinit binary:

```bash
# Build both binaries
cargo build --release

# Install zinit-http
sudo cp target/release/zinit-http /usr/local/bin/
```

## Usage

### Starting the HTTP Proxy

```bash
# Start with default settings (port 8080, Unix socket at /var/run/zinit.sock)
zinit-http

# Start with custom settings
zinit-http --socket /path/to/zinit.sock --port 3000 --bind 0.0.0.0
```

### Command Line Options

| Option | Description |
|--------|-------------|
| `--socket` | Path to the Zinit Unix socket (default: `/var/run/zinit.sock`) |
| `--port` | HTTP port to listen on (default: `8080`) |
| `--bind` | IP address to bind to (default: `127.0.0.1`) |
| `--help` | Show help information |

## Making API Requests

Once the HTTP proxy is running, you can send JSON-RPC requests to it:

```bash
curl -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"service.list","params":[]}' \
  http://localhost:8080/
```

The API protocol is identical to the Unix socket protocol - see the [API Documentation](api.md) for details on available methods and parameters.

## Security Considerations

By default, `zinit-http` binds only to `127.0.0.1`, making it accessible only from the local machine. When exposing the API remotely:

1. Consider running behind a reverse proxy with TLS (like Nginx or Traefik)
2. Implement authentication if needed (through the reverse proxy)
3. Use firewall rules to restrict access to trusted hosts

## Running as a Service

You can set up `zinit-http` as a Zinit-managed service:

```yaml
# /etc/zinit/zinit-http.yaml
exec: "/usr/local/bin/zinit-http --port 8080"
after:
  - zinit
```

This allows Zinit to manage its own HTTP proxy, providing a complete self-contained solution.

## Troubleshooting

If you encounter issues with the HTTP proxy:

1. Ensure Zinit is running and its Unix socket exists
2. Check that the port isn't already in use by another application
3. Verify network connectivity and firewall rules if accessing remotely
4. Look for error messages in the terminal where zinit-http was started