# Zinit Architecture Diagram

This document provides a visual overview of Zinit's architecture.

## High-Level Architecture

```
                           ┌───────────────┐
                           │  Zinit Init   │
                           │   (PID 1)     │
                           └───────┬───────┘
                                   │
                 ┌─────────────────┼─────────────────┐
                 │                 │                 │
        ┌────────▼─────┐  ┌────────▼─────┐  ┌────────▼─────┐
        │              │  │              │  │              │
        │  Service     │  │  Socket API  │  │    Log       │
        │  Manager     │  │  Server      │  │    Manager   │
        │              │  │              │  │              │
        └────────┬─────┘  └──────┬───────┘  └──────────────┘
                 │               │
      ┌──────────┘               │
      │                          │
┌─────▼────────┐          ┌──────▼──────┐
│              │          │             │
│  Services    │          │  Client     │
│              │          │  Commands   │
└──────────────┘          └─────────────┘
```

## Component Overview

### 1. Core Components

- **Zinit Init (PID 1)**
  - The main process that runs as PID 1
  - Orchestrates all other components
  - Handles system signals

- **Service Manager**
  - Starts and monitors services
  - Manages service lifecycle
  - Handles dependencies between services
  - Manages service state transitions

- **Socket API Server**
  - Listens on Unix socket
  - Accepts commands from clients
  - Forwards commands to Service Manager
  - Returns responses to clients

- **Log Manager**
  - Captures service output
  - Routes logs to appropriate destinations
  - Provides log retrieval functionality

### 2. Client Interface

- **Client Commands**
  - Connects to the Unix socket
  - Sends commands to the API Server
  - Displays results to users

### 3. Services

- **Service Processes**
  - Actual processes managed by Zinit
  - Each service has its own configuration
  - Services can depend on other services

## Data Flow

1. **Configuration Loading**
   - Read YAML files from config directory
   - Parse service definitions

2. **Service Start Sequence**
   - Resolve dependencies
   - Start services in dependency order
   - Test service health
   - Update service state

3. **Command Processing**
   - Client sends command to socket
   - API server processes command
   - Service manager takes action
   - Result returned to client

4. **Service Monitoring**
   - Watch for service exits
   - Restart services when needed
   - Update dependencies

## System States

- **Startup**: Loading configs, starting services
- **Running**: Normal operation, monitoring services
- **Shutdown**: Stopping services in reverse dependency order

## Key Files

- **src/main.rs**: Command parsing and dispatching
- **src/zinit/mod.rs**: Core service management
- **src/app/api.rs**: Socket API implementation
- **src/manager/mod.rs**: Process management

This architecture makes Zinit:
- Lightweight: Focused on just the essentials
- Reliable: Clear service lifecycle management
- Extensible: Simple communication protocol