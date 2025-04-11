# Working with Zinit's JSON-RPC Protocol

This document provides a detailed guide on how to communicate with Zinit using its JSON-RPC 2.0 protocol, including connection methods, message formatting, and practical examples.

## Understanding JSON-RPC

JSON-RPC is a stateless, lightweight remote procedure call (RPC) protocol that uses JSON for data encoding. Zinit implements JSON-RPC 2.0, which has a specific message format for requests and responses.

### Why JSON-RPC?

Zinit has moved from a legacy line-based protocol to JSON-RPC because it provides:

- Structured, well-defined message format
- Better error handling
- Support for named parameters
- Ability to batch multiple requests
- Wider tool and language support

## Connecting to Zinit

### Unix Socket Connection

The primary method to connect to Zinit is through its Unix socket, typically located at `/var/run/zinit.sock`:

Using `curl`:
```bash
curl --unix-socket /var/run/zinit.sock -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"service.list","params":[]}' \
  http://localhost/
```

Using `socat`:
```bash
echo '{"jsonrpc":"2.0","id":1,"method":"service.list","params":[]}' | \
  socat - UNIX-CONNECT:/var/run/zinit.sock
```

Using `nc` (netcat):
```bash
echo -e "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: $(echo -n '{"jsonrpc":"2.0","id":1,"method":"service.list","params":[]}' | wc -c)\r\n\r\n$(echo -n '{"jsonrpc":"2.0","id":1,"method":"service.list","params":[]}')" | \
  nc -U /var/run/zinit.sock
```

### HTTP Connection (via zinit-http)

For remote access or when Unix socket access isn't available, use the `zinit-http` proxy:

```bash
curl -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"service.list","params":[]}' \
  http://localhost:8080/
```

## JSON-RPC Message Format

### Request Format

A basic JSON-RPC request has this structure:

```json
{
  "jsonrpc": "2.0",      // Protocol version (always "2.0")
  "id": 1,               // Request identifier (client-defined)
  "method": "service.name", // Method to call
  "params": []           // Parameters (array or object based on method)
}
```

### Response Format

A successful response:

```json
{
  "jsonrpc": "2.0",      // Protocol version
  "id": 1,               // Same ID as the request
  "result": {}           // Result data (varies by method)
}
```

An error response:

```json
{
  "jsonrpc": "2.0",      // Protocol version
  "id": 1,               // Same ID as the request
  "error": {             // Error information
    "code": -32000,      // Error code
    "message": "Error message",
    "data": "Additional error details"
  }
}
```

## Practical Examples

Let's look at practical examples for common Zinit operations:

### Listing All Services

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "service.list",
  "params": []
}
```

**Bash example:**
```bash
curl --unix-socket /var/run/zinit.sock -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"service.list","params":[]}' \
  http://localhost/
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "redis": "Running",
    "nginx": "Running",
    "backup": "Success"
  }
}
```

### Getting Service Status

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "service.status",
  "params": ["nginx"]
}
```

**Bash example:**
```bash
curl --unix-socket /var/run/zinit.sock -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"service.status","params":["nginx"]}' \
  http://localhost/
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "name": "nginx",
    "pid": 1234,
    "state": "Running",
    "target": "Up",
    "after": {
      "network": "Success"
    }
  }
}
```

### Starting a Service

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "service.start",
  "params": ["nginx"]
}
```

**Bash example:**
```bash
curl --unix-socket /var/run/zinit.sock -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"service.start","params":["nginx"]}' \
  http://localhost/
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": null
}
```

### Stopping a Service

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "service.stop",
  "params": ["nginx"]
}
```

**Bash example:**
```bash
curl --unix-socket /var/run/zinit.sock -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"service.stop","params":["nginx"]}' \
  http://localhost/
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": null
}
```

### Creating a New Service

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "service.create",
  "params": ["webserver", {
    "exec": "/usr/bin/nginx -g 'daemon off;'",
    "oneshot": false,
    "after": ["network"],
    "env": {
      "NGINX_OPTS": "-c /etc/nginx/custom.conf"
    }
  }]
}
```

**Bash example:**
```bash
curl --unix-socket /var/run/zinit.sock -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "id":1,
    "method":"service.create",
    "params":["webserver", {
      "exec": "/usr/bin/nginx -g \"daemon off;\"",
      "oneshot": false,
      "after": ["network"],
      "env": {
        "NGINX_OPTS": "-c /etc/nginx/custom.conf"
      }
    }]
  }' \
  http://localhost/
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": "Service 'webserver' created"
}
```

### System Reboot

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "system.reboot",
  "params": []
}
```

**Bash example:**
```bash
curl --unix-socket /var/run/zinit.sock -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"system.reboot","params":[]}' \
  http://localhost/
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": null
}
```

## Error Handling

When an operation fails, Zinit returns an error response. Here's an example of trying to start a non-existent service:

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "service.start",
  "params": ["nonexistent"]
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32000,
    "message": "Service not found",
    "data": "service name \"nonexistent\" unknown"
  }
}
```

Common error codes:

| Code    | Message           | Description                              |
|---------|-------------------|------------------------------------------|
| -32000  | Service not found | The specified service does not exist     |
| -32001  | Service already monitored | Service is already being monitored |
| -32002  | Service is up     | Operation requires service to be stopped |
| -32003  | Service is down   | Operation requires service to be running |

## API Discovery

Zinit supports API discovery through the `rpc.discover` method. This returns the full OpenRPC specification:

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "rpc.discover",
  "params": []
}
```

**Response:** The complete OpenRPC specification document describing all available methods.

## Programmatic Clients

While direct socket or HTTP communication works well for scripts and manual operations, for application integration, consider using:

1. The official `zinit-client` Rust library
2. Language-specific JSON-RPC client libraries

## Legacy Protocol

Note that the older line-based protocol is no longer supported. All clients should use the JSON-RPC protocol described in this document.