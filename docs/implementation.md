# Implementation
zinit uses tokio and async io to manage services and processes. The idea is as follows

- zinit starts an event loop (process manager)
- the loop waits for `messages` to process
- a `message` describes a certain `event` (something has happened), or an `action` (ask manager to do something)
- possible events (so far) 
  - service has exited 
  - service state has changed
- possible actions (so far)
  - monitor service
  - re-spawn service
  - star/stop service
  - get service status
  - list available service
- once an message is received the process manager takes proper action to satisfy this request.
- since process manager has a single running event loop, locking is not needed, and other data race are not possible.

## Life cycle
- The monitor action is the only way to tell the process manager about a new service. Once received by the event loop, the process will get spawned (if all its dependencies are already running)
  - if the service defines a `test` command. the test command is executed repeatedly with incremental delays in between until it returns `success` then the service is marked as `running`. if the number of trials are exceeded the service is marked as `test-fail` state.
  - if one of the service dependencies are not marked as running (or success) the service is marked as `blocked`, and parked, we don't spawn it at this time.
- If a service process stops for any reason (success, or error) another event is received (`exit`)
- The service state is updated with success, or error.
- If the service is `oneshot` no further action is taken 
- If the service is not `oneshot`, a delay is introduced before the service is re-spawned.
- Anytime a service state is updated to `running` or `success` the dependency tree is re-evaluated to figure out which services can be unblocked and started.

## Ctrl cycle
- Start/Stop commands only works on a `monitored` service.
- On stop, the service target state is set to `down` and then signalled with `SIGTERM`. The signal can be changed as per configuration
- On start, the service target state is set to `up` and only re-spawned if it wasn't running.
- Status/List commands are similar but only read the state flag associated with the service(s).
- Forgetting a service can be applied only on a 'stopped' service.