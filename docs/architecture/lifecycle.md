# Service Lifecycle

This document describes the lifecycle of services managed by Zinit, including the various states a service can be in and how services transition between those states.

## Service State Diagram

```
                        ┌───────────┐
                        │           │
                        │  Unknown  │
                        │           │
                        └─────┬─────┘
                              │
                              │ Monitor
                              ▼
                        ┌───────────┐
                        │           │◄────────── Dependencies
                        │  Blocked  │            not met
                        │           │
                        └─────┬─────┘
                              │
                              │ Dependencies met
                              ▼
                        ┌───────────┐
          Start         │           │
     ◄──────────────────┤  Spawned  │
     │                  │           │
     │                  └─────┬─────┘
     │                        │
     │                        │ Test succeeds
     │                        ▼
     │                  ┌───────────┐
     │                  │           │
     │                  │  Running  ├───────────┐
     │                  │           │           │
     │                  └─────┬─────┘           │
     │                        │                 │
     │  Process exits         │ Process exits   │
     │  (one-shot)            │ (non one-shot)  │ Stop/Kill
     │                        │                 │
     ▼                        ▼                 │
┌───────────┐           ┌───────────┐           │
│           │           │           │           │
│  Success  │           │   Error   │◄──────────┘
│           │           │           │
└───────────┘           └───────────┘
```

## Service States

Zinit services can exist in one of several states:

### 1. Unknown
The initial state of a service when it is first monitored but before any action has been taken. This state indicates that Zinit doesn't yet have information about the service's status.

### 2. Blocked
The service is waiting for its dependencies to be satisfied. A service enters this state when one or more of the services listed in its `after` configuration are not yet in a Running or Success state.

### 3. Spawned
The service process has been started but either:
- The test command hasn't been run yet
- The test command is still being retried
- There is no test command and Zinit is waiting to see if the process exits immediately

### 4. Running
The service is actively running and considered healthy. A service enters this state when:
- Its test command has succeeded
- It has been running long enough to be considered stable (if no test command exists)

### 5. Success
The service process has exited with a successful status code (0). This state is primarily meaningful for oneshot services, which are expected to exit.

### 6. Error
The service process has exited with a non-zero status code, indicating failure. Services that aren't oneshot will be restarted after entering this state.

### 7. TestFailure
The service's test command has failed repeatedly, indicating that although the process is running, it isn't functioning correctly.

### 8. Failure
The service has failed to start in a way that retry won't help (e.g., command not found, permission denied). This is a terminal state until configuration is changed.

## Target States

In addition to the current state, every service has a target state:

### Up
The service should be running. If it exits, Zinit will try to restart it (unless it's a oneshot service).

### Down
The service should be stopped. If it's running, Zinit will stop it and won't restart it if it exits.

## State Transitions

### Monitoring a Service
1. Service configuration is loaded from its YAML file
2. Service enters the Unknown state
3. Zinit checks if dependencies are satisfied
   - If yes, the service is spawned
   - If no, the service enters the Blocked state

### Dependency Resolution
1. When a service enters Running or Success state, Zinit evaluates all services in the Blocked state
2. Any Blocked services whose dependencies are now met will proceed to be spawned

### Starting a Service
1. Process is spawned with the exec command
2. Service enters the Spawned state
3. If a test command exists:
   - Zinit runs the test command repeatedly until it succeeds
   - When the test succeeds, the service enters the Running state
4. If no test command exists:
   - For oneshot services, the service is immediately marked as Running
   - For non-oneshot services, the service is marked Running once spawned

### Service Process Exit
1. When a service process exits, Zinit captures its exit status
2. The service state is updated:
   - Success state if the exit code was 0
   - Error state if the exit code was non-zero
3. Next steps depend on service type:
   - For oneshot services, no further action is taken
   - For non-oneshot services with target state Up, the service will be restarted after a delay
   - For services with target state Down, no restart is attempted

### Stopping a Service
1. The service's target state is set to Down
2. The configured stop signal (default: SIGTERM) is sent to the process
3. The service remains in its current state until the process exits
4. Once the process exits, it transitions to Success or Error state based on exit code
5. No restart is attempted because the target state is Down

### Killing a Service
1. The specified signal is sent to the process
2. The service remains in its current state
3. If the signal causes the process to exit:
   - The service transitions to Success or Error state based on exit code
   - If target state is Up, the service will be automatically restarted

### Forgetting a Service
1. The service must already have target state Down and no running process
2. The service is removed from Zinit's management
3. All state information about the service is discarded

## Automatic Restart Behavior

For non-oneshot services with target state Up:

1. When a service exits (regardless of exit code), Zinit waits for a delay period (default: 2 seconds)
2. After the delay, Zinit attempts to restart the service
3. If the service fails to start, this cycle repeats indefinitely

The delay prevents rapid restart loops that could overload the system.

## System Shutdown Process

During system shutdown:

1. All services' target states are set to Down
2. Services are stopped in reverse dependency order:
   - Build a reversed dependency graph
   - Stop services with no dependents first
   - Continue in topological order
3. Each service is given time to shut down gracefully (based on shutdown_timeout)
4. After all services are stopped, the system poweroff or reboot is executed