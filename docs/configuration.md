# Zinit Service Configuration

Zinit uses YAML files for service configuration. This document explains the format and provides examples for common use cases.

## Configuration Location

By default, Zinit looks for service configuration files in the `/etc/zinit/` directory. You can specify a different directory using the `--config` flag:

```bash
zinit init --config /path/to/config/dir
```

Each service is defined in its own YAML file, with the filename (minus the `.yaml` extension) becoming the service name.

## Configuration Format

A basic service configuration file contains the command to run and optional parameters:

```yaml
# Basic service configuration
exec: "/usr/bin/my-service --option value"   # Command to execute (required)
oneshot: false                               # Whether to restart on exit
after:                                       # Dependencies
  - network
  - database
```

## Configuration Options

| Option | Type | Description |
|--------|------|-------------|
| `exec` | String | Command to execute (required) |
| `oneshot` | Boolean | If false (default), service will be restarted when it exits. If true, it will run once and not be restarted. |
| `after` | Array | List of services that must be running before this one starts |
| `log` | String | Log output handling: "null" (discard), "ring" (in-memory buffer), or "stdout" (forward to stdout) |
| `env` | Object | Environment variables for the service |
| `shutdown_timeout` | Integer | Maximum time (in seconds) to wait for service to stop during shutdown |
| `test` | String | Health check command - if provided, tests service health |

## Examples

### Basic Service

```yaml
# /etc/zinit/nginx.yaml
exec: "/usr/sbin/nginx -g 'daemon off;'"
```

### One-Shot Service

A service that runs once and doesn't restart:

```yaml
# /etc/zinit/db-setup.yaml
exec: "/usr/bin/setup-database"
oneshot: true
after:
  - postgresql
```

### Service with Dependencies

A service that waits for other services:

```yaml
# /etc/zinit/web-app.yaml
exec: "/usr/bin/web-app"
after:
  - redis
  - postgres
  - nginx
```

### Service with Environment Variables

```yaml
# /etc/zinit/api-server.yaml
exec: "/usr/bin/api-server"
env:
  PORT: "3000"
  DB_HOST: "localhost"
  DB_USER: "appuser"
  LOG_LEVEL: "info"
```

### Service with Custom Logging

```yaml
# /etc/zinit/chatbot.yaml
exec: "/usr/bin/chatbot-service"
log: "ring"  # Store logs in a ring buffer
```

## Creating Services via JSON-RPC

Services can also be created programmatically using the JSON-RPC API:

```bash
curl --unix-socket /var/run/zinit.sock -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "service.create",
    "params": ["webserver", {
      "exec": "/usr/bin/nginx",
      "oneshot": false,
      "after": ["network"],
      "env": {
        "NGINX_OPTS": "-c /etc/nginx/custom.conf"
      }
    }]
  }' \
  http://localhost/