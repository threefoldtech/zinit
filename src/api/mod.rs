use failure::Error;
use future;
use nix::sys::signal;
use shlex;
use std::str::FromStr;
use std::{fs, path};
use tokio::codec::{Framed, LinesCodec};
use tokio::net::{UnixListener, UnixStream};
use tokio::prelude::*;

use crate::manager::{Handle, State};

const SOCKET_NAME: &str = "zinit.socket";

type Result<T> = std::result::Result<T, Error>;

fn list(handle: Handle) -> Result<Option<String>> {
    let mut result = String::new();
    for service in handle.list() {
        result.push_str(&service);
        let process = handle.status(service)?;
        result.push_str(&format!(": {:?}", process.state));
        result.push('\n');
    }
    result.pop();
    Ok(Some(result))
}

fn status(args: &[String], handle: Handle) -> Result<Option<String>> {
    let mut result = String::new();
    if args.len() != 1 {
        bail!("invalid arguments, expecting one argument service name 'status <service>'")
    }

    let process = handle.status(&args[0])?;
    result += &format!("name: {}\n", &args[0]);
    result += &format!("pid: {}\n", process.pid);
    result += &format!("state: {:?}\n", process.state);
    result += &format!("target: {:?}\n", process.target);
    if process.config.after.len() > 0 {
        result += &format!("after: \n");
    }
    for dep in process.config.after {
        let state = match handle.status(&dep) {
            Ok(dep) => dep.state,
            Err(_) => State::Unknown,
        };
        result += &format!(" - {}: {:?}\n", dep, state);
    }
    result.pop();
    Ok(Some(result))
}

fn stop(args: &[String], handle: Handle) -> Result<Option<String>> {
    if args.len() != 1 {
        bail!("invalid arguments, expecting one argument service name 'stop <service>'")
    }

    handle.stop(&args[0])?;
    Ok(None)
}

fn kill(args: &[String], handle: Handle) -> Result<Option<String>> {
    if args.len() != 2 {
        bail!("invalid arguments, expecting two argument 'stop <service> SIGNAL'")
    }

    let sig = signal::Signal::from_str(&args[1].to_uppercase())?;
    handle.kill(&args[0], sig)?;
    Ok(None)
}

fn start(args: &[String], handle: Handle) -> Result<Option<String>> {
    if args.len() != 1 {
        bail!("invalid arguments, expecting one argument service name 'start <service>'")
    }

    handle.start(&args[0])?;
    Ok(None)
}

fn forget(args: &[String], handle: Handle) -> Result<Option<String>> {
    if args.len() != 1 {
        bail!("invalid arguments, expecting one argument service name 'forget <service>'")
    }

    handle.forget(&args[0])?;
    Ok(None)
}

fn process_cmd(handle: Handle, cmd: Vec<String>) -> Result<Option<String>> {
    if cmd.len() == 0 {
        bail!("missing command");
    }

    match cmd[0].as_ref() {
        "list" => list(handle),
        "status" => status(&cmd[1..], handle),
        "stop" => stop(&cmd[1..], handle),
        "start" => start(&cmd[1..], handle),
        "kill" => kill(&cmd[1..], handle),
        "forget" => forget(&cmd[1..], handle),
        _ => Err(format_err!("unknown command")),
    }
}

fn process(handle: Handle, socket: UnixStream) {
    let framed = Framed::new(socket, LinesCodec::new());
    let (sink, stream) = framed.split();

    let future = stream
        .map_err(|_| ())
        .fold(sink, move |sink, line| {
            let handle = handle.clone();
            let cmd = shlex::split(&line);
            let result = match cmd {
                Some(cmd) => process_cmd(handle, cmd),
                None => Err(format_err!("invalid command")),
            };

            future::done(result).then(|answer| {
                let mut buffer = String::new();
                // Yes, it's intended to look like http header
                // to support adding more metadata in the future
                // currently the only header we have is STATUS
                // also this allow the protocol to be very simply
                // and human readable at the same time.
                match answer {
                    Ok(answer) => {
                        buffer.push_str("STATUS: OK\r\n");
                        buffer.push_str("\r\n");
                        match answer {
                            Some(answer) => buffer.push_str(&answer),
                            None => (),
                        };
                    }
                    Err(err) => {
                        buffer.push_str("STATUS: ERROR\r\n");
                        buffer.push_str("\r\n");
                        buffer.push_str(&format!("{}", err));
                    }
                }

                sink.send(buffer)
                    .map_err(|e| eprintln!("failed to send answer: {}", e))
            })
        })
        .map(|_| println!("end of stream"));

    tokio::spawn(future);
}

pub fn run(handle: Handle) -> Result<()> {
    let p = path::Path::new("/var/run");
    fs::create_dir_all(p)?;
    let p = p.join(SOCKET_NAME);
    let _ = fs::remove_file(&p);

    let server = UnixListener::bind(p)?
        .incoming()
        .map_err(|e| error!("accept err: {}", e))
        .for_each(move |socket| {
            let handle = handle.clone();
            process(handle, socket);
            Ok(())
        });

    tokio::spawn(server);
    Ok(())
}
