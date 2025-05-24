# Service Stats Functionality

This document describes the stats functionality in Zinit, which provides memory and CPU usage information for services and their child processes.

## Overview

The stats functionality allows you to monitor the resource usage of services managed by Zinit. It provides information about:

- Memory usage (in bytes)
- CPU usage (as a percentage)
- Child processes and their resource usage

This is particularly useful for monitoring system resources and identifying services that might be consuming excessive resources.

## Command Line Usage

To get stats for a service using the command line:

```bash
zinit stats <service-name>
```

Example:
```bash
zinit stats nginx
```

This will output YAML-formatted stats information:

```yaml
name: nginx
pid: 1234
memory_usage: 10485760  # Memory usage in bytes (10MB)
cpu_usage: 2.5          # CPU usage as percentage
children:               # Stats for child processes
  - pid: 1235
    memory_usage: 5242880
    cpu_usage: 1.2
  - pid: 1236
    memory_usage: 4194304
    cpu_usage: 0.8
```

## JSON-RPC API

The stats functionality is also available through the JSON-RPC API:

### Method: `service_stats`

Get memory and CPU usage statistics for a service.

**Parameters:**
- `name` (string, required): The name of the service to get stats for

**Returns:**
- Object containing stats information:
  - `name` (string): Service name
  - `pid` (integer): Process ID of the service
  - `memory_usage` (integer): Memory usage in bytes
  - `cpu_usage` (number): CPU usage as a percentage (0-100)
  - `children` (array): Stats for child processes
    - Each child has:
      - `pid` (integer): Process ID of the child process
      - `memory_usage` (integer): Memory usage in bytes
      - `cpu_usage` (number): CPU usage as a percentage (0-100)

**Example Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "service_stats",
  "params": {
    "name": "nginx"
  }
}
```

**Example Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "name": "nginx",
    "pid": 1234,
    "memory_usage": 10485760,
    "cpu_usage": 2.5,
    "children": [
      {
        "pid": 1235,
        "memory_usage": 5242880,
        "cpu_usage": 1.2
      },
      {
        "pid": 1236,
        "memory_usage": 4194304,
        "cpu_usage": 0.8
      }
    ]
  }
}
```

**Possible Errors:**
- `-32000`: Service not found
- `-32003`: Service is down

## Implementation Details

The stats functionality works by:

1. Reading process information from `/proc/<pid>/` directories on Linux systems
2. Calculating memory usage from `/proc/<pid>/status` (VmRSS field)
3. Calculating CPU usage by sampling `/proc/<pid>/stat` over a short interval
4. Identifying child processes by checking the PPid field in `/proc/<pid>/status`

On non-Linux systems, the functionality provides placeholder values as the `/proc` filesystem is specific to Linux.

## Notes

- Memory usage is reported in bytes
- CPU usage is reported as a percentage (0-100)
- The service must be running to get stats (otherwise an error is returned)
- Child processes are identified by their parent PID matching the service's PID