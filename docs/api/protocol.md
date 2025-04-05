# Zinit API Protocol

This document describes the wire protocol used to communicate with a running Zinit instance. The protocol allows external applications to control and query Zinit services.

## Protocol Overview

Zinit uses a simple line-based text protocol over a Unix domain socket. The protocol follows a request-response pattern similar to HTTP 1.0:

1. Client connects to the Unix socket
2. Client sends a command (terminated by newline)
3. Server processes the command
4. Server sends a response
5. Server closes the connection

## Socket Location

By default, Zinit listens on a Unix domain socket at:

```
/var/run/zinit.sock
```

This path can be changed using the `-s` or `--socket` option when starting Zinit.

## Protocol Format

### Requests

Requests are single-line text commands sent to the socket:

```
<command> [arguments...]
```

Each request must end with a newline character (`\n`).

### Responses

Responses are JSON-formatted data (except for the `log` command). The general response format is:

```json
{
  "state": "ok|error",
  "body": <response-data>
}
```

Where:
- `state`: Indicates whether the command succeeded (`ok`) or failed (`error`)
- `body`: Contains the command-specific response data or error message

## Available Commands

### `list`

Lists all services managed by Zinit.

**Request:**
```
list
```

**Response (success):**
```json
{
  "state": "ok",
  "body": {
    "service1": "Running",
    "service2": "Success",
    "service3": "Error"
  }
}
```

### `status <service>`

Shows detailed status information for a specific service.

**Request:**
```
status <service>
```

**Response (success):**
```json
{
  "state": "ok",
  "body": {
    "name": "redis",
    "pid": 1234,
    "state": "Running",
    "target": "Up",
    "after": {
      "dependency1": "Success",
      "dependency2": "Running"
    }
  }
}
```

**Response (error):**
```json
{
  "state": "error",
  "body": "service name \"unknown\" unknown"
}
```

### `start <service>`

Starts a service.

**Request:**
```
start <service>
```

**Response (success):**
```json
{
  "state": "ok",
  "body": null
}
```

### `stop <service>`

Stops a service.

**Request:**
```
stop <service>
```

**Response (success):**
```json
{
  "state": "ok",
  "body": null
}
```

### `restart <service>`

Restarts a service.

**Request:**
```
restart <service>
```

**Response (success):**
```json
{
  "state": "ok",
  "body": null
}
```

### `monitor <service>`

Starts monitoring a service. The service configuration is loaded from the config directory.

**Request:**
```
monitor <service>
```

**Response (success):**
```json
{
  "state": "ok",
  "body": null
}
```

**Response (error - already monitored):**
```json
{
  "state": "error",
  "body": "service \"redis\" already monitored"
}
```

### `forget <service>`

Stops monitoring a service. You can only forget a stopped service.

**Request:**
```
forget <service>
```

**Response (success):**
```json
{
  "state": "ok",
  "body": null
}
```

**Response (error - service is running):**
```json
{
  "state": "error",
  "body": "service \"redis\" is up"
}
```

### `kill <service> <signal>`

Sends a signal to a running service.

**Request:**
```
kill <service> <signal>
```

Where `<signal>` is a Unix signal name (e.g., SIGTERM, SIGKILL).

**Response (success):**
```json
{
  "state": "ok",
  "body": null
}
```

### `shutdown`

Stops all services and powers off the system.

**Request:**
```
shutdown
```

**Response (success):**
```json
{
  "state": "ok",
  "body": null
}
```

### `reboot`

Stops all services and reboots the system.

**Request:**
```
reboot
```

**Response (success):**
```json
{
  "state": "ok",
  "body": null
}
```

### `log [snapshot]`

Returns a stream of log lines from all services.

**Request (all logs, follow):**
```
log
```

**Request (all logs, snapshot only):**
```
log snapshot
```

**Response:**

Unlike other commands, the `log` command returns plain text rather than JSON. Each line is a log entry.

When used with the `snapshot` argument, Zinit returns all current logs and then closes the connection. Without this argument, it returns all current logs and then keeps the connection open, streaming new log entries as they appear.

## Client Example: Using nc (netcat)

Since Zinit uses a simple line protocol, you can easily interact with it using the `nc` (netcat) command:

```bash
# List all services
echo "list" | sudo nc -U /var/run/zinit.sock

# Check status of redis
echo "status redis" | sudo nc -U /var/run/zinit.sock

# Start the nginx service
echo "start nginx" | sudo nc -U /var/run/zinit.sock

# Get a snapshot of all logs
echo "log snapshot" | sudo nc -U /var/run/zinit.sock
```

## Client Example: Using Bash Function

Here's a simple bash function to interact with Zinit:

```bash
zinit_call() {
  cmd="$1"
  echo "$cmd" | sudo nc -U /var/run/zinit.sock
}

# Usage examples
zinit_call "list"
zinit_call "status redis"
zinit_call "start nginx"
```

## Client Example: Using Python

A simple Python client:

```python
import socket
import json

def zinit_call(command):
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect("/var/run/zinit.sock")
    sock.send((command + "\n").encode())
    
    response = sock.recv(4096).decode()
    sock.close()
    
    # For log command, return raw text
    if command.startswith("log"):
        return response
    
    # Parse JSON for other commands
    return json.loads(response)

# Usage examples
services = zinit_call("list")
redis_status = zinit_call("status redis")
start_result = zinit_call("start nginx")
```

## Error Handling

When a command fails, Zinit returns a response with `state` set to `error` and `body` containing an error message:

```json
{
  "state": "error",
  "body": "error message here"
}
```

Common error scenarios include:
- Unknown service name
- Service already in the requested state
- Service already monitored/not monitored
- Invalid signal name
- Permission issues with the socket

## Security Considerations

The Unix socket by default is owned by root and requires appropriate permissions to access. Make sure any client applications have the necessary permissions to communicate with the socket.