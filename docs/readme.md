# Command line interface
`zinit` provides sub commands to run as init process, and ctrl commands to start, stop, and query running services.
Run `zinit --help` to show a list of the current implemented sub-commands.

## init
the init sub-command is the main mode for zinit. 
It reads the configured services files in available in the config directory`/etc/zinit` or anopther one provided by the`-c` flag. Then it auto monitors those services.

When a service is monitored, it means that it's auto started, and then watched in case the service exited for any reason. When a service exits, it's automatically restarted, unless it's marked as a `oneshot`

## Running zinit in a container

When running zinit in a container, supply the `--container` argument to the init command.

If you want to read more about `zinit` process manager please check [here](implementation.md)

### Service configuration
```yaml
exec: "command line to start service"
test: "command line to test service is running" # optional
oneshot: true or false (false by default)
after: # list of services that we depend on (optional)
   - service1_name
   - service2_name
signal: # optional section
  stop: SIGKILL # the signal sent on `stop` action. default to SIGTERM
log: null | ring | stdout
env:
  KEY: VALUE
```

- `oneshot` service is not going to re-spawn when it exits.
- if a service depends on a `oneshot` services, it will not get started, unless the oneshot service exits with success.
- if a service depends on another service (that is not `oneshot`), it will not get started, unless the service is marked as `running`
- a service with no test command is marked running if it successfully executed, regardless if it exits immediately after or not, hence a test command is useful.
- If a test command is provided, the service will not consider running, unless the test command pass
- You can override the stop signal sent to the service as shown in the example. Currently only the stop
  signal can be overwritten. More signal types might be added in the future (for example, reload).
- the log directive can be set to one of the following values
  - `null`: ignore all service logs (like `> /dev/null`)
  - `ring`: the default value, which means all logs of the service is written to the kernel ring buffer. The name is service is prepended to the log line.
  - `stdout`: print the output on zinit stdout
- env (dict) is an extra set of env variables (KEY, VALUE) paris that would be available on a service

> Note: to use `ring` inside docker make sure you add the `kmsg` device to the list of allowed devices
```
docker run -dt --device=/dev/kmsg:/dev/kmsg:wm zinit
```

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

## Controlling commands
```bash
zinit --help
```

```
zinit 0.1
ThreeFold Tech, https://github.com/threefoldtech
A runit replacement

USAGE:
    zinit [SUBCOMMAND]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

SUBCOMMANDS:
    forget     forget a service. you can only forget a stopped service
    help       Prints this message or the help of the given subcommand(s)
    init       run in init mode, start and maintain configured services
    kill       send a signal to a running service.
    list       quick view of current known services and their status
    log        view services logs from zinit ring buffer
    monitor    start monitoring a service. configuration is loaded from server config directory
    start      start service. has no effect if the service is already running
    status     show detailed service status
    stop       stop service

```

As already described above, once zinit starts in init mode, it auto monitor all services configured under the provided configuration directory. Once a service is 'monitored' you can control it with one of the following commands.

- `kill`: Similar to the unix `kill` command, it sends a signal to a named service (default to `sigterm`). If the signal terminates the service, `zinit` will auto start it since the service target state is still `up`
- `stop`: Stop sets the target state of the service to `down`, and send the `stop` signal. The stop signal is defaulted to `sigterm` but can be overwritten in the service configuration file. A `stop` action doesn't wait for the service to exit nor grantee that it's killed. It grantees that once the service is down, it won't re-spawn. A caller to the `stop` action can poll on the service state until it's down, or decide to send another signal (for example `kill <service> SIGKILL`) to fully stop it.
- `start`: start is the opposite of `stop`. it will set the target state to `up` and will re-spawn the service if it's not already running.
- `status`: shows detailed status of a named service.
- `forget`: works only on a `stopped` service (means that the target state is set to `down` by a previous call to `stop`). Also no process must be associated with the service (if the `stop` call didn't do it, a `kill` might)
- `list`: show a quick view of all monitored services.
- `log`: show services logs from the zinit ring buffer. The buffer size is configured in `init`
- `monitor`: monitor will load config of a service `name` from the configuration directory. and monitor it, this will allow you to add new
service to the configuration directory in runtime.

## Config Files
zinit does not require any other config files other that the service unit files. But zinit respects some of the global unix standard files:
- `/etc/environment` . The file is read one time during boot, changes to this file in runtime has no effect (even for new services)
