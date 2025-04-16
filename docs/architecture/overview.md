# Zinit Architecture Overview

This document provides a high-level overview of the Zinit architecture and how its components work together.

## Core Components

Zinit consists of several key components that work together to provide service management:

```
                  ┌─────────────────────┐
                  │      Zinit Init     │
                  │   (Process Manager) │
                  └──────────┬──────────┘
                             │
              ┌──────────────┼──────────────┐
              │              │              │
     ┌────────▼─────┐ ┌──────▼───────┐ ┌───▼────────────┐
     │              │ │              │ │                │
     │ Service      │ │ Unix Socket  │ │ Log Management │
     │ Management   │ │ API Server   │ │                │
     │              │ │              │ │                │
     └──────┬───────┘ └──────┬───────┘ └────────────────┘
            │                │
     ┌──────▼───────┐        │
     │              │        │
     │ Dependency   │   ┌────▼─────┐
     │ Resolution   │   │          │
     │              │   │ Client   │
     └──────────────┘   │ Commands │
                        │          │
                        └──────────┘
```

### 1. Process Manager

The central component of Zinit is the Process Manager which:
- Manages the lifecycle of service processes
- Monitors their status
- Handles their output logging
- Reacts to service exits by restarting them when appropriate

The Process Manager uses an event loop to respond to events like service state changes or external commands.

### 2. Service Management

The Service Management component handles:
- Service configuration parsing
- Service status tracking
- Service state transitions
- Service process spawning and monitoring

Services exist in different states (Unknown, Blocked, Spawned, Running, Success, Error, etc.) and transition between them based on events.

### 3. Dependency Resolution

The Dependency Resolution component:
- Builds a Directed Acyclic Graph (DAG) of service dependencies
- Determines which services can start based on their dependencies
- Blocks services with unmet dependencies
- Unblocks services when their dependencies are satisfied

Zinit uses a topological sorting algorithm to properly order service startup and shutdown.

### 4. Unix Socket API Server

The Unix Socket API Server:
- Listens on a Unix socket (default: `/var/run/zinit.sock`)
- Accepts commands from clients
- Forwards commands to the Process Manager
- Returns results to clients in JSON format

This provides a simple, language-agnostic interface for controlling Zinit.

### 5. Log Management

The Log Management component:
- Captures output from services
- Routes logs based on configuration (null, ring buffer, stdout)
- Provides log retrieval via the API

### 6. Client Commands

The Client Commands component:
- Provides a CLI for interacting with Zinit
- Connects to the Unix socket
- Sends commands and displays responses
- Formats output for human readability

## Key Traits

Zinit's design includes several important traits:

1. **Async I/O**: Zinit uses Tokio for asynchronous I/O operations, allowing it to manage many services efficiently.

2. **State-driven**: Services have both a current state (what they're doing now) and a target state (what they should be doing).

3. **Non-blocking**: The service manager doesn't block while waiting for operations to complete.

4. **Self-healing**: Services are automatically restarted when they exit unexpectedly.

5. **Event-driven**: The system is built around an event processing loop that reacts to service changes and external commands.

## Data Flow

1. **Configuration Reading**:
   - Service configurations are read from YAML files
   - Configurations are parsed into Service structures
   - Services are added to the manager's tracking table

2. **Service Startup**:
   - Dependencies are checked before starting a service
   - If dependencies are met, the service process is spawned
   - Test commands are executed to verify service health
   - Service state is updated based on outcomes

3. **Service Monitoring**:
   - Process exits are detected and handled
   - Services are restarted unless they're oneshot or marked down
   - State changes trigger dependency recalculation

4. **Command Processing**:
   - Commands arrive via the Unix socket
   - The Process Manager acts on commands
   - Results are returned via the socket
   - Service state may change as a result

## System Interactions

### 1. Process Control
Zinit uses standard Unix signals to control processes:
- `SIGTERM` (default) or custom signals for stopping
- `SIGKILL` for forceful termination when needed

### 2. System Shutdown
During system shutdown:
- Services are stopped in reverse dependency order
- A timeout ensures services get a chance to exit gracefully
- Log and socket servers are closed
- System reboot/poweroff is triggered

### 3. Container Mode
In container mode:
- Zinit behaves differently on shutdown
- It exits normally instead of powering off the system
- It responds to container termination signals

## Memory and Resource Model

Zinit maintains a lightweight memory footprint:
- Service configurations are loaded on demand
- Log buffering is configurable
- Tokio's async model allows efficient handling of many services
- State is stored in memory with no external database requirements