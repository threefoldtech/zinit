# Zinit JSON-RPC API Documentation

This document describes the JSON-RPC 2.0 API for Zinit. The API allows external applications to control and query Zinit services using the JSON-RPC 2.0 protocol.

> **Note:** The JSON-RPC API is currently in development and may not be fully functional in all Zinit versions. For production use, it's recommended to use the [line-based protocol](protocol.md) which is stable and well-tested.

## OpenRPC Specification

The OpenRPC specification for Zinit's API is exists in the [openrpc.json](../../openrpc.json) file. You can copy this JSON and paste it into the [OpenRPC Playground](https://playground.open-rpc.org/) to explore the API interactively.


## Using the OpenRPC Playground

To explore the Zinit JSON-RPC API interactively:

1. Visit [OpenRPC Playground](https://playground.open-rpc.org/)
2. Copy the JSON specification above
3. Paste it into the editor on the left side of the playground
4. Use the interactive documentation on the right side to explore the API

## JSON-RPC 2.0 Protocol

Zinit implements the JSON-RPC 2.0 protocol as defined in the [JSON-RPC 2.0 Specification](https://www.jsonrpc.org/specification).

### Request Format

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "service.list",
  "params": {}
}
```

Where:
- `jsonrpc`: Must be exactly "2.0"
- `id`: A unique identifier for the request (can be a number, string, or null)
- `method`: The name of the method to call
- `params`: An object containing the parameters for the method (can be omitted for methods that don't require parameters)

### Response Format

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {}
}
```

Or, in case of an error:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32000,
    "message": "Service not found",
    "data": "service name \"unknown\" unknown"
  }
}
```

Where:
- `jsonrpc`: Always "2.0"
- `id`: The same id as in the request
- `result`: The result of the method call (only present if the call was successful)
- `error`: Error information (only present if the call failed)
  - `code`: A number indicating the error type
  - `message`: A short description of the error
  - `data`: Additional information about the error (optional)

## Client Example: Using Python

A simple Python client for the JSON-RPC API (note that this may not work with all Zinit versions):

```python
import socket
import json

def zinit_jsonrpc(method, params=None):
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect("/var/run/zinit.sock")
    
    request = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": method
    }
    
    if params:
        request["params"] = params
    
    # Send the JSON-RPC request without a newline
    sock.send(json.dumps(request).encode())
    
    response = sock.recv(4096).decode()
    sock.close()
    
    return json.loads(response)

# Usage examples
services = zinit_jsonrpc("service.list")
redis_status = zinit_jsonrpc("service.status", {"name": "redis"})
start_result = zinit_jsonrpc("service.start", {"name": "nginx"})
```

## Error Codes

Zinit uses standard JSON-RPC 2.0 error codes as well as custom error codes:

### Standard JSON-RPC 2.0 Error Codes

- `-32600`: Invalid Request - The JSON sent is not a valid Request object
- `-32601`: Method not found - The method does not exist / is not available
- `-32602`: Invalid params - Invalid method parameter(s)
- `-32603`: Internal error - Internal JSON-RPC error

### Custom Zinit Error Codes

- `-32000`: Service not found - The requested service does not exist
- `-32001`: Service already monitored - The service is already being monitored
- `-32002`: Service is up - The service is currently running
- `-32003`: Service is down - The service is currently not running
- `-32004`: Invalid signal - The provided signal is invalid
- `-32005`: Config error - Error in service configuration
- `-32006`: Shutting down - The system is already shutting down