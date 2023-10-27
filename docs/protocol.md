# zinit API Protocol

This documents describes the wire protocol used by the zinit command line tool to talk to the running (zinit init) instance. Basically when you run `zinit init` as your init (normally PID 1) it also listens on a `unix` socket that is available by default under `/var/run/zinit.sock`.

When you execute a command (for example `zinit list`) the tool will connect to the socket, send command to your running instance of `init` to read/write information.

The protocol is a very simple line protocol. the process always like

- connect(unix, /var/run/zinit.sock)
- write(`<command>`), where command has to always end with a (newline) `\n`
- read() read response of the operation

after reading the response the connection is automatically closed from the server side (similar to HTTP 1). To do another operation you will need to open a new connection, send command and read the results.

## Controlling zinit with `nc`

Since zinit uses a simple line protocol you can very easily use `nc` tool to connect to zinit and control it. Once you have a working `zinit init` process running (preferably also running some processes) try the following

```bash
sudo nc -U /var/run/zinit.sock
```

nc will now wait for you to send a `command` to zinit.

Now type `list` then press enter, you should get output similar to

```json
{"state":"ok","body":{"redis":"Running"}}
```

so seems `redis` is running. Now do

```bash
sudo nc -U /var/run/zinit.sock
status redis
```

It should print something like

```json
{"state":"ok","body":{"after":{"delay":"Success"},"name":"redis","pid":320996,"state":"Running","target":"Up"}}
```

## Available commands

- `list`
- `status <serivce>`
- `start <service>`
- `stop <service>`
- `forget <service>`
- `monitor <service>`
- `kill <service>`
- `shutdown`
- `reboot`
- `log [snapshot]`

> NOTE: the log command will return a stream of all logs in plain text format. if `log` has `snapshot` argument it will read all logs available in zinit ringbuffer, then exits (connection is closed from server side). If `log` without a snapshot, it will first print all logs from log ringbuffer, then will follow the logs forever (connection is kept open) and all new logs will appear on the stream as they happen.

For details and documentation of what the commands do please refer to docs of [commands](readme.md#controlling-commands)

## Returned json schema

All commands (except `log`) return a very json structure:

```json
{
    "state": "ok|error",
    "body": <body>
}
```

In case of `state == error` body will be the error message. If `state == ok` then body is command specific data.

> #TODO: document output for each command body
