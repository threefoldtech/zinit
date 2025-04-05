# Implementation Details

This document provides an in-depth look at Zinit's implementation, its components, and how they work together. It's intended for developers interested in understanding Zinit's internals or contributing to the project.

## Overall Architecture

Zinit is implemented in Rust using the Tokio async runtime. Its architecture consists of several key components:

```
                  ┌─────────────────────┐
                  │      ZInit Main     │
                  │    (src/main.rs)    │
                  └──────────┬──────────┘
                             │
              ┌──────────────┼──────────────┐
              │              │              │
     ┌────────▼─────┐ ┌──────▼───────┐ ┌───▼────────────┐
     │              │ │              │ │                │
     │ ZInit Core   │ │ API Server   │ │ Process Manager│
     │ (src/zinit)  │ │ (src/app)    │ │ (src/manager)  │
     └──────────────┘ └──────────────┘ └────────────────┘
```

## Core Components

### Main (src/main.rs)

The main module handles:
- Command line argument parsing using Clap
- Dispatching to the appropriate app function based on the command
- Error handling for the top-level application

### ZInit Core (src/zinit)

The core Zinit module implements the service management logic:

- **Service State Machine**: Handles service state transitions
- **Dependency Management**: Resolves service dependencies
- **Lifecycle Management**: Starts, stops, and monitors services

Key structures:
- `ZInit`: The main service manager
- `ZInitService`: Represents a single service
- `State`: Enum for service states
- `Target`: Enum for service target states (Up/Down)

### Process Manager (src/manager)

The Process Manager is responsible for:
- Starting and managing child processes
- Handling process output (stdout/stderr)
- Managing process signals
- Collecting exit status information

### API Server (src/app)

The API module implements the Unix socket server that:
- Listens for commands
- Processes command requests
- Returns results to clients
- Formats data for human consumption

## Concurrency Model

Zinit uses Tokio for async I/O operations, which allows it to manage many services efficiently without creating a thread per service. The concurrency model works as follows:

1. The main event loop runs in a single thread
2. Service spawning and management is handled asynchronously
3. Message passing is used for communication between components
4. Tokio tasks (lightweight coroutines) are used for parallel operations

## Service Management

### Service Configuration (src/zinit/config.rs)

The configuration module handles:
- Reading YAML configuration files
- Parsing service definitions
- Validating service configuration
- Loading service directory contents

The `Service` struct contains all service configuration options:
- Execution command 
- Test command
- One-shot flag
- Shutdown timeout
- Dependencies
- Signal handling preferences
- Logging configuration
- Environment variables
- Working directory

### Service Ordering (src/zinit/ord.rs)

The ordering module manages:
- Building a Directed Acyclic Graph (DAG) of service dependencies
- Topological sorting for proper startup and shutdown order
- Cycle detection in dependency relationships

### Service State Management (src/zinit/mod.rs)

Service state management includes:
- Tracking current and target states
- Handling state transitions
- Reacting to process exits
- Coordinating dependency-based startup

## Process Handling

The process handling logic (in src/manager):

1. Creates child processes with proper environment
2. Sets up stdout/stderr redirection based on logging settings
3. Monitors processes for termination
4. Handles process group management
5. Delivers signals to processes

## Communication Protocol

The Unix socket protocol (implemented in src/app/api.rs):

1. Accepts connections from clients
2. Parses line-based commands
3. Dispatches commands to the appropriate handlers
4. Formats responses as JSON
5. Returns results to clients

## Logging System

Zinit offers multiple logging options:

1. **Ring Buffer**: A circular buffer that stores a configurable number of recent log lines
2. **Stdout**: Direct output to Zinit's stdout
3. **Null**: Discards output

## Shutdown Process

The shutdown process works as follows:

1. Set the global shutdown flag
2. Build a dependency graph for shutdown order
3. Stop services in reverse dependency order
4. Wait for services to exit (with timeout)
5. Sync filesystem caches
6. Trigger system reboot/poweroff

## Container Mode

In container mode:
1. Listen for termination signals (SIGTERM, SIGINT, SIGHUP)
2. On signal, gracefully stop services
3. Exit the process instead of triggering system power actions

## Code Structure

```
zinit/
├── src/
│   ├── main.rs           # CLI interface, command parsing
│   ├── app/
│   │   ├── mod.rs        # Command implementation
│   │   └── api.rs        # Unix socket server
│   ├── manager/
│   │   ├── mod.rs        # Process manager
│   │   └── buffer.rs     # Ring buffer for logs
│   └── zinit/
│       ├── mod.rs        # Core service management
│       ├── config.rs     # Configuration parsing
│       └── ord.rs        # Dependency resolution
├── docs/                 # Documentation
└── docker/               # Docker test environment
```

## Key Algorithms

### Dependency Resolution

Zinit uses the following algorithm for dependency resolution:

1. Build a Directed Acyclic Graph (DAG) with services as nodes and dependencies as edges
2. Perform topological sorting to determine startup order
3. During system operation:
   - When a service state changes to Running or Success
   - Re-evaluate which blocked services can now start
   - Launch newly unblocked services

### Shutdown Orchestration

During shutdown:

1. Reverse the dependency graph
2. Start with services that no other services depend on
3. Once those services are stopped, move to the next layer
4. Continue until all services are stopped

### Service Testing

For services with test commands:

1. After starting a service, run its test command
2. If the test fails, wait and retry
3. Once the test succeeds, mark the service as Running
4. If test repeatedly fails, mark the service as TestFailure

## Memory Management

Zinit uses Rust's ownership model for memory safety:

1. Service state is owned by the service manager
2. Arc (Atomic Reference Counting) and RwLock (Read-Write lock) are used for shared ownership and thread safety
3. Most data structures are immutable once created
4. Tokio's message passing minimizes the need for shared mutable state

## Error Handling

Error handling in Zinit follows these patterns:

1. Most functions return `Result<T>` to indicate success or failure
2. The `anyhow` crate is used for general error handling
3. Domain-specific errors use the `thiserror` crate (e.g., `ZInitError`)
4. Low-level errors are wrapped in context to make them more meaningful
5. Errors are logged with appropriate severity levels

## Conclusion

Zinit's implementation emphasizes:

1. Simplicity: Focusing on the core service management functionality
2. Reliability: Using Rust's ownership model for memory safety
3. Efficiency: Leveraging async I/O for scalable service management
4. Clarity: Following clear design patterns throughout the codebase

These principles make Zinit a lightweight yet powerful init system suitable for both system-level and container use cases.