# Zinit JSON-RPC API

Zinit provides a JSON-RPC 2.0 API over a Unix socket, allowing programmatic control of services and the system. This document explains how to connect to Zinit and build RPC messages to interact with it.

## Connection Methods

There are multiple ways to connect to Zinit's API:

### 1. Unix Socket (Direct)

Unix socket communication is the primary and most efficient method for local access:

```bash
curl --unix-socket /var/run/zinit.sock -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"service.list","params":[]}' \
  http://localhost/
```

### 2. HTTP Proxy (Remote Access)

For remote access or when Unix socket access isn't suitable, the `zinit-http` proxy provides HTTP access:

```bash
# First, start the HTTP proxy
zinit-http --socket /var/run/zinit.sock --port 8080

# Then connect via HTTP
curl -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"service.list","params":[]}' \
  http://localhost:8080/
```

### 3. Client Library

For Rust applications, use the `zinit-client` library (see [Client Library Documentation](../zinit-client/README.md)).

## JSON-RPC Message Format

All API interactions use the JSON-RPC 2.0 format:

```json
{
  "jsonrpc": "2.0",      // JSON-RPC version (always "2.0")
  "id": 1,               // Request ID (client-defined)
  "method": "method.name", // Method to call
  "params": []           // Parameters (array or object based on method)
}
```

Response format:

```json
{
  "jsonrpc": "2.0",      // JSON-RPC version
  "id": 1,               // Matching the request ID
  "result": {}           // Result data (on success)
}
```

Or on error:

```json
{
  "jsonrpc": "2.0",      // JSON-RPC version
  "id": 1,               // Matching the request ID
  "error": {             // Error information
    "code": -32000,      // Error code
    "message": "Error message",
    "data": "Additional error details"
  }
}
```

## Available Methods

### Service Management

#### service.list

Lists all services and their states.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "service.list",
  "params": []
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "service1": "Running",
    "service2": "Success",
    "service3": "Error"
  }
}
```

#### service.status

Gets detailed status for a specific service.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "service.status",
  "params": ["redis"]
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
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

#### service.start

Starts a service.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "service.start",
  "params": ["redis"]
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": null
}
```

#### service.stop

Stops a service.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "service.stop",
  "params": ["redis"]
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": null
}
```

#### service.monitor

Starts monitoring a service from its configuration.

**Request:**
```json
{
  "jsonrpc": "2.0", 
  "id": 1,
  "method": "service.monitor",
  "params": ["redis"]
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": null
}
```

#### service.forget

Stops monitoring a service. Service must be stopped first.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "service.forget",
  "params": ["redis"]
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": null
}
```

#### service.kill

Sends a signal to a running service.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "service.kill",
  "params": ["redis", "SIGTERM"]
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": null
}
```

### Service Configuration

#### service.create

Creates a new service configuration.

**Request:**
```json
{
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
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": "Service 'webserver' created"
}
```

#### service.delete

Deletes a service configuration.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "service.delete",
  "params": ["webserver"]
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": "Service 'webserver' deleted"
}
```

#### service.get

Gets a service configuration.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "service.get",
  "params": ["webserver"]
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "exec": "/usr/bin/nginx",
    "oneshot": false,
    "after": ["network"],
    "env": {
      "NGINX_OPTS": "-c /etc/nginx/custom.conf"
    }
  }
}
```

### System Management

#### system.shutdown

Stops all services and powers off the system.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "system.shutdown",
  "params": []
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": null
}
```

#### system.reboot

Stops all services and reboots the system.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "system.reboot",
  "params": []
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": null
}
```

## Error Codes

| Code    | Message           | Description                              |
|---------|-------------------|------------------------------------------|
| -32000  | Service not found | The specified service does not exist     |
| -32001  | Service already monitored | Service is already being monitored |
| -32002  | Service is up     | Operation requires service to be stopped |
| -32003  | Service is down   | Operation requires service to be running |
| -32004  | Invalid signal    | The specified signal is not valid        |
| -32005  | Config error      | Error in service configuration           |
| -32006  | Shutting down     | System is already in shutdown process    |
| -32007  | Service already exists | Cannot create duplicate service     |
| -32008  | Service file error | Error accessing service configuration file |

## API Discovery

The API supports self-discovery via the `rpc.discover` method:

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "rpc.discover",
  "params": []
}
```

**Response:** Returns the OpenRPC specification for the API.