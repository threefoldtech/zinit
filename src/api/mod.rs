use crate::ring::RingLog;
use failure::Error;
use future;
use nix::sys::signal;
use shlex;
use snafu::{ResultExt, Snafu}; // 0.2.3
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use tokio::codec::{Framed, LinesCodec};
use tokio::net::{UnixListener, UnixStream};
use tokio::prelude::*;

use crate::manager::{Handle, State};
use crate::settings::load;

pub const SOCKET_NAME: &str = "zinit.sock";
pub const RINGLOG_NAME: &str = "log.sock";

type Result<T> = std::result::Result<T, Error>;
type PathResult<T> = std::result::Result<T, ProgramError>;

#[derive(Debug, Snafu)]
pub enum ProgramError {
    #[snafu(display("{}: {}", path.display(), source))]
    Metadata { source: io::Error, path: PathBuf },
}

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
    if !process.config.after.is_empty() {
        result += &"after: \n".to_string();
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

fn monitor(args: &[String], handle: Handle) -> Result<Option<String>> {
    if args.len() != 1 {
        bail!("invalid arguments, expecting one argument service name 'forget <service>'")
    }

    let (name, service) = load(format!("{}.yaml", args[0]))?;

    handle.monitor(name, service)?;
    Ok(None)
}

fn process_cmd(handle: Handle, cmd: Vec<String>) -> Result<Option<String>> {
    if cmd.is_empty() {
        bail!("missing command");
    }

    match cmd[0].as_ref() {
        "list" => list(handle),
        "status" => status(&cmd[1..], handle),
        "stop" => stop(&cmd[1..], handle),
        "start" => start(&cmd[1..], handle),
        "kill" => kill(&cmd[1..], handle),
        "forget" => forget(&cmd[1..], handle),
        "monitor" => monitor(&cmd[1..], handle),
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
                let mut header = String::new();
                // the line protocol is very simple and
                // kinda looks like http. this will allow
                // to add extra meta data to the response
                // later.
                // the current header we have now are
                // - status: ok || error
                // - lines: number of lines of body to read
                // headers are separated from the content by
                // 1 empty line
                let msg = match answer {
                    Ok(answer) => {
                        header.push_str("status: ok\n");
                        match answer {
                            Some(answer) => answer,
                            None => String::new(),
                        }
                    }
                    Err(err) => {
                        header.push_str("status: error\n");
                        format!("{}", err)
                    }
                };

                let lines = msg.lines().count();

                header.push_str(&format!("lines: {}\n", lines));
                sink.send(header)
                    .and_then(|sink| match msg.len() {
                        0 => future::Either::A(future::ok(sink)),
                        _ => future::Either::B(sink.send(msg)),
                    })
                    .map_err(|e| eprintln!("failed to send answer: {}", e))
            })
        })
        .map(|_| ());

    tokio::spawn(future);
}

pub fn run(handle: Handle) -> PathResult<()> {
    let path = Path::new("/var/run");
    fs::create_dir_all(path).context(Metadata { path })?;
    let path = path.join(SOCKET_NAME);
    let _ = fs::remove_file(&path);

    let server = UnixListener::bind(&path)
        .context(Metadata { path })?
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

pub fn logd(log: Arc<RingLog>) -> PathResult<()> {
    let path = Path::new("/var/run");
    fs::create_dir_all(path).context(Metadata { path })?;
    let path = path.join(RINGLOG_NAME);
    let _ = fs::remove_file(&path);

    let server = UnixListener::bind(&path)
        .context(Metadata { path })?
        .incoming()
        .map_err(|e| error!("accept err: {}", e))
        .for_each(move |socket| {
            let d = log.as_ref();
            tokio::spawn(d.sink(socket));
            Ok(())
        });

    tokio::spawn(server);
    Ok(())
}
