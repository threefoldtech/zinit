use failure::Error;
use future;
use shlex;
use std::{fs, path};
use tokio::codec::{Framed, LinesCodec};
use tokio::net::{UnixListener, UnixStream};
use tokio::prelude::*;

use crate::manager::Handle;

const SOCKET_NAME: &str = "zinit.socket";

type Result<T> = std::result::Result<T, Error>;

fn list(handle: Handle) -> Result<String> {
    let mut result = String::new();
    for service in handle.list() {
        result.push_str(&service);
        let status = handle.status(service)?;
        result.push_str(&format!(": {:?}", status));
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

    let status = handle.status(&args[0])?;
    result.push_str(&format!("{:?}", status));
    Ok(result)
}

fn process_cmd(handle: Handle, cmd: Vec<String>) -> impl Future<Item = String, Error = Error> {
    if cmd.len() == 0 {
        return future::Either::B(future::err(format_err!("missing command")));
    }

    let answer = match cmd[0].as_ref() {
        "list" => list(handle),
        "status" => status(&cmd[1..], handle),
        // "hello" => future::ok(format!("hi {:?}", &cmd[1..])),
        // "say" => future::ok(format!("{:?}", &cmd[1..])),
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
