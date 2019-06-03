# Command line interface
`zinit` provides sub commands to run as init process, and ctrl commands to start, stop, and query running services.
Run `zinit --help` to show a list of the current implemented sub-commands.

## init
the init sub-command is the main mode for zinit. it reads the configured services files available under the config directory provided by the `-c` flag. Then it auto monitor those services.

When a service is monitored, it means that it's auto started, and then watched in case the service exited for any reason. When a service exits, it's automatically restarted, unless it's marked as a `oneshot`

If you want to read more about `zinit` process manager please check [here](implementation.md)

### Service configuration
```yaml
exec: "command line to start service"
test: "command line to test service is running" # optional
oneshot: true or false (false by default)
after: # list of services that we depend on (optional)
   - service1_name
   - service2_name

``` 

- `oneshot` service is not going to re-spawn when it exits.
- if a service depends on a `oneshot` services, it will not get started, unless the oneshot service exits with success.
- if a service depends on another service (that is not `oneshot`), it will not get started, unless the service is marked as `running`
- a service with no test command is marked running if it successfully executed, regardless if it exits immediately after or not, hence a test command is useful.
- If a test command is provided, the service will not consider running, unless the test command pass

#### Examples
redis-init.yaml
```yaml
exec: sh -c "echo 'prepare db files for redis!'"
oneshot: true
```

redis.yaml
```yaml
exec: redis-server --port 7777
test: redis-cli -p 7777 PING
after:
  - redis-init
```

redis-after.yaml
```yaml
exec: sh -c "populate redis with seed data"
oneshot: true
after:
  - redis
```