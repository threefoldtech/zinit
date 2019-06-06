use failure::Error;
use future;
use shlex;
use std::{fs, path};
use tokio::codec::{Framed, LinesCodec};
use tokio::net::{UnixListener, UnixStream};
use tokio::prelude::*;

use crate::manager::{Handle, State};

const SOCKET_NAME: &str = "zinit.socket";
const DONE: &str = "done";

type Result<T> = std::result::Result<T, Error>;

fn list(handle: Handle) -> Result<String> {
    let mut result = String::new();
    for service in handle.list() {
        result.push_str(&service);
        let process = handle.status(service)?;
        result.push_str(&format!(": {:?}", process.state));
        result.push('\n');
    }
    result.pop();
    Ok(result)
}

fn status(args: &[String], handle: Handle) -> Result<String> {
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
    Ok(result)
}

fn stop(args: &[String], handle: Handle) -> Result<String> {
    if args.len() != 1 {
        bail!("invalid arguments, expecting one argument service name 'stop <service>'")
    }

    handle.stop(&args[0])?;
    Ok(DONE.to_string())
}

fn start(args: &[String], handle: Handle) -> Result<String> {
    if args.len() != 1 {
        bail!("invalid arguments, expecting one argument service name 'start <service>'")
    }

    handle.start(&args[0])?;
    Ok(DONE.to_string())
}

fn process_cmd(handle: Handle, cmd: Vec<String>) -> impl Future<Item = String, Error = Error> {
    if cmd.len() == 0 {
        return future::Either::B(future::err(format_err!("missing command")));
    }

    let answer = match cmd[0].as_ref() {
        "list" => list(handle),
        "status" => status(&cmd[1..], handle),
        "stop" => stop(&cmd[1..], handle),
        "start" => start(&cmd[1..], handle),
        _ => Err(format_err!("unknown command")),
    };
    match answer {
        Ok(answer) => future::Either::A(future::ok(answer)),
        Err(err) => future::Either::B(future::err(err)),
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
            let future = match cmd {
                Some(cmd) => future::Either::A(process_cmd(handle, cmd)),
                None => future::Either::B(future::ok(String::from("invalid command"))),
            };

            future.then(|answer| {
                let mut buffer = String::new();
                match answer {
                    Ok(answer) => {
                        buffer.push_str("STATUS: OK");
                        buffer.push_str("\r\n\r\n");
                        buffer.push_str(&answer);
                    }
                    Err(err) => {
                        buffer.push_str("STATUS: ERROR\n");
                        buffer.push_str("\r\n\r\n");
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
