# Zinit Command Line Reference

This document provides a comprehensive reference for all Zinit command line options and commands.

## Command Structure

Zinit uses a command-based CLI with the following general structure:

```bash
zinit [FLAGS] [OPTIONS] [SUBCOMMAND]
```

## Global Flags and Options

These flags and options apply to all Zinit commands:

| Flag/Option | Description |
|-------------|-------------|
| `-d, --debug` | Run in debug mode with increased verbosity |
| `-h, --help` | Display help information |
| `-V, --version` | Display version information |
| `-s, --socket <PATH>` | Path to Unix socket (default: `/var/run/zinit.sock`) |

## Subcommands

### Main Mode

#### `init`

Run Zinit in init mode, starting and maintaining configured services.

```bash
zinit init [FLAGS] [OPTIONS]
```

**Flags:**
- `--container`: Run in container mode, exiting on signal instead of rebooting

**Options:**
- `-c, --config <DIR>`: Service configurations directory (default: `/etc/zinit/`)
- `-b, --buffer <SIZE>`: Buffer size (in lines) to keep service logs (default: `2000`)

**Example:**
```bash
# Run in init mode with custom config directory
zinit init -c /opt/services/

# Run in container mode
zinit init --container
```

### Service Management

#### `list`

Display a quick view of all currently known services and their status.

```bash
zinit list
```

**Output:**
A JSON object with service names as keys and their status as values.

**Example:**
```bash
# List all services
zinit list
```

#### `status`

Show detailed status information for a specific service.

```bash
zinit status <SERVICE>
```

**Arguments:**
- `<SERVICE>`: Name of the service to show status for

**Example:**
```bash
# Check status of redis service
zinit status redis
```

#### `start`

Start a service. Has no effect if the service is already running.

```bash
zinit start <SERVICE>
```

**Arguments:**
- `<SERVICE>`: Name of the service to start

**Example:**
```bash
# Start the nginx service
zinit start nginx
```

#### `stop`

Stop a service. Sets the target state to "down" and sends the stop signal.

```bash
zinit stop <SERVICE>
```

**Arguments:**
- `<SERVICE>`: Name of the service to stop

**Example:**
```bash
# Stop the redis service
zinit stop redis
```

#### `restart`

Restart a service. If it fails to stop, it will be killed and then started again.

```bash
zinit restart <SERVICE>
```

**Arguments:**
- `<SERVICE>`: Name of the service to restart

**Example:**
```bash
# Restart the web service
zinit restart web
```

#### `monitor`

Start monitoring a service. The configuration is loaded from the server's config directory.

```bash
zinit monitor <SERVICE>
```

**Arguments:**
- `<SERVICE>`: Name of the service to monitor

**Example:**
```bash
# Monitor the database service
zinit monitor database
```

#### `forget`

Remove a service from monitoring. You can only forget a stopped service.

```bash
zinit forget <SERVICE>
```

**Arguments:**
- `<SERVICE>`: Name of the service to forget

**Example:**
```bash
# Forget the backup service
zinit forget backup
```

#### `kill`

Send a signal to a running service.

```bash
zinit kill <SERVICE> <SIGNAL>
```

**Arguments:**
- `<SERVICE>`: Name of the service to send signal to
- `<SIGNAL>`: Signal name (e.g., SIGTERM, SIGKILL, SIGINT)

**Example:**
```bash
# Send SIGTERM to the redis service
zinit kill redis SIGTERM

# Send SIGKILL to force terminate a service
zinit kill stuck-service SIGKILL
```

### System Operations

#### `shutdown`

Stop all services in dependency order and power off the system.

```bash
zinit shutdown
```

**Example:**
```bash
# Shutdown the system
zinit shutdown
```

#### `reboot`

Stop all services in dependency order and reboot the system.

```bash
zinit reboot
```

**Example:**
```bash
# Reboot the system
zinit reboot
```

### Logging

#### `log`

View service logs from the Zinit ring buffer.

```bash
zinit log [FLAGS] [FILTER]
```

**Flags:**
- `-s, --snapshot`: If set, log prints current buffer without following

**Arguments:**
- `[FILTER]`: Optional service name to filter logs for

**Examples:**
```bash
# View logs for all services and follow new logs
zinit log

# View current logs for the nginx service without following
zinit log -s nginx
```

## Exit Codes

Zinit commands return the following exit codes:

| Code | Description |
|------|-------------|
| 0 | Success |
| 1 | Error (with error message printed to stderr) |

## Environment Variables

Zinit respects the following environment variables:

- `/etc/environment`: Read at startup to populate the environment for all services

## Examples

### Basic Service Management

```bash
# Start the zinit daemon in init mode
sudo zinit init

# List all services
zinit list

# Monitor a new service
zinit monitor myservice

# Check service status
zinit status myservice

# Stop a service
zinit stop myservice

# Remove a service from monitoring
zinit stop myservice
zinit forget myservice
```

### Container Usage

```bash
# Start zinit in container mode
zinit init --container -c /app/services/

# Monitor a service
zinit monitor app
```

### Manual Service Control

#### Using Line-Based Protocol with nc

Zinit supports a simple line-based protocol that you can interact with using netcat:

```bash
# List services
echo "list" | sudo nc -U /var/run/zinit.sock

# Start a service
echo "start redis" | sudo nc -U /var/run/zinit.sock