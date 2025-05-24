# Service Configuration Format

This document describes the structure and options for Zinit service configuration files.

## File Format

Zinit uses YAML files for service configuration. Each service has its own configuration file stored in the Zinit configuration directory (default: `/etc/zinit`).

### File Naming and Location

- **Location**: `/etc/zinit/` (default, can be changed with `-c` flag)
  - on osx `~/hero/cfg/zinit`
- **Naming**: `<service-name>.yaml`

For example:
- `/etc/zinit/nginx.yaml`
- `/etc/zinit/redis.yaml`

## Configuration Schema

Service configuration files use the following schema:

```yaml
# Command to run (required)
exec: "command line to start service"

# Command to test if service is running (optional)
test: "command line to test service"

# Whether the service should be restarted (optional, default: false)
oneshot: true|false

# Maximum time to wait for service to stop during shutdown (optional, default: 10)
shutdown_timeout: 30

# Services that must be running before this one starts (optional)
after:
  - service1_name
  - service2_name

# Signals configuration (optional)
signal:
  stop: SIGKILL  # signal sent on 'stop' action (default: SIGTERM)

# Log handling configuration (optional, default: ring)
log: null|ring|stdout

# Environment variables for the service (optional)
env:
  KEY1: "VALUE1"
  KEY2: "VALUE2"

# Working directory for the service (optional)
dir: "/path/to/working/directory"
```

## Configuration Options

### Required Fields

| Field | Description |
|-------|-------------|
| `exec` | Command line to execute when starting the service |

### Optional Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `test` | String | - | Command to determine if service is running |
| `oneshot` | Boolean | `false` | If true, service won't be restarted after exit |
| `shutdown_timeout` | Integer | 10 | Seconds to wait for service to stop during shutdown |
| `after` | String[] | `[]` | List of services that must be running first |
| `signal.stop` | String | `"sigterm"` | Signal to send when stopping the service |
| `log` | Enum | `ring` | How to handle service output (null, ring, stdout) |
| `env` | Object | `{}` | Environment variables to pass to the service |
| `dir` | String | `""` | Working directory for the service |

## Field Details

### exec

The command to run when starting the service. This is the only required field in the configuration.

```yaml
exec: "/usr/bin/redis-server --port 6379"
```

Shell-style commands are supported:

```yaml
exec: "sh -c 'echo Starting service && /usr/local/bin/myservice'"
```

### test

Command that tests whether the service is running properly. Zinit runs this command periodically until it succeeds (exit code 0), at which point the service is considered running.

```yaml
test: "redis-cli -p 6379 PING"
```

If no test command is provided, the service is considered running as soon as it's started.

### oneshot

When set to `true`, the service will not be automatically restarted when it exits. This is useful for initialization tasks or commands that should run only once.

```yaml
oneshot: true
```

Services that depend on a oneshot service will start only after the oneshot service has exited successfully.

### shutdown_timeout

How long (in seconds) to wait for the service to stop during system shutdown before giving up:

```yaml
shutdown_timeout: 30  # Wait up to 30 seconds
```

### after

List of service names that must be running (or completed successfully for oneshot services) before this service starts:

```yaml
after:
  - networking
  - database
```

### signal

Custom signals to use for operations. Currently, only the `stop` signal is configurable:

```yaml
signal:
  stop: SIGKILL  # Use SIGKILL instead of default SIGTERM
```

Valid signal names follow the standard UNIX signal naming (SIGTERM, SIGKILL, SIGINT, etc).

### log

How to handle stdout/stderr output from the service:

```yaml
log: stdout  # Print output to zinit's stdout
```

Options:
- `null`: Ignore all service output (like redirecting to /dev/null)
- `ring`: Store logs in the kernel ring buffer with service name prefix (default)
- `stdout`: Send service output to zinit's stdout

> **Note**: To use `ring` inside Docker, make sure to add the `kmsg` device:
> ```
> docker run -dt --device=/dev/kmsg:/dev/kmsg:rw zinit
> ```

### env

Additional environment variables for the service. These are added to the existing environment:

```yaml
env:
  PORT: "8080"
  DEBUG: "true"
  NODE_ENV: "production"
```

### dir

Working directory for the service process:

```yaml
dir: "/var/lib/myservice"
```

If not specified, the process inherits zinit's working directory.

## Example Configurations

### Web Server

```yaml
exec: "/usr/bin/nginx -g 'daemon off;'"
test: "curl -s http://localhost > /dev/null"
after:
  - networking
log: stdout
```

### Database Initialization

```yaml
exec: "sh -c 'echo Creating database schema && /usr/bin/db-migrate'"
oneshot: true
dir: "/opt/myapp"
env:
  DB_HOST: "localhost"
  DB_USER: "admin"
```

### Application with Dependencies

```yaml
exec: "/usr/bin/myapp --config /etc/myapp.conf"
test: "curl -s http://localhost:8080/health > /dev/null"
after:
  - database
  - cache
signal:
  stop: SIGINT  # Use SIGINT for graceful shutdown
env:
  PORT: "8080"
shutdown_timeout: 20