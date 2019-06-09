use crate::api;
use crate::manager;
use crate::settings;

use failure::Error;
use future::lazy;
use std::collections::HashMap;
use std::io::{self, BufRead};
use std::os::unix::net;
use std::path;
use tokio::prelude::*;

type Result<T> = std::result::Result<T, Error>;

/// init start init command, immediately monitor all services
/// that are defined under the config directory
pub fn init(config: &str, debug: bool) -> Result<()> {
    // load config
    if !debug && std::process::id() != 1 {
        bail!("can only run as pid 1");
    }

    std::env::set_current_dir(config)?;

    let configs = settings::load_dir(".", |file, err| {
        println!(
            "encountered err {} while loading file {:?}. skipping!",
            err, file
        );
        settings::Walk::Continue
    })?;

    // start the tokio runtime, start the process manager
    // and monitor all configured services
    // TODO:
    // We need to start the unix socket server that will
    // receive and handle user management commands (start, stop, status, etc...)
    tokio::run(lazy(|| {
        // creating a new instance from the process manager
        let manager = manager::Manager::new();

        // running the manager returns a handle that we can
        // use to actually control the process manager
        // currently the handle only exposes one method
        // `monitor` which spawns a new task on the process
        // manager given the configuration
        let handle = manager.run();

        for (name, config) in configs.into_iter() {
            if let Err(err) = handle.monitor(name, config) {
                error!("failed to monitor service: {}", err);
            }
        }

        if let Err(e) = api::run(handle) {
            error!("failed to start ctrl api {}", e);
        }

        Ok(())
    }));

    Ok(())
}

/// Status is response status from server
/// Only OK, and ERROR are available now
enum Status {
    Ok,
    Error,
}

impl std::str::FromStr for Status {
    type Err = Error;
    fn from_str(s: &str) -> Result<Status> {
        match s.trim().to_lowercase().as_ref() {
            "ok" => Ok(Status::Ok),
            "error" => Ok(Status::Error),
            _ => bail!("unknown status"),
        }
    }
}

/// Line protocol implementation
trait APIProtocol: io::Read + io::Write {
    fn request(&mut self, cmd: &str) -> Result<String>;
}

impl APIProtocol for net::UnixStream {
    fn request(&mut self, cmd: &str) -> Result<String> {
        let mut headers = HashMap::new();
        self.write_all(cmd.as_bytes())?;
        self.write_all(b"\n")?;
        //let lines = read.lines();
        let mut read = io::BufReader::new(self);
        loop {
            let mut line = String::new();
            read.read_line(&mut line)?;
            let line = line.trim();
            if line == "" {
                //end of headers section
                break;
            }

            let parts: Vec<&str> = line.splitn(2, ":").collect();
            if parts.len() != 2 {
                bail!("invalid header syntax");
            }

            headers.insert(parts[0].trim().to_string(), parts[1].trim().to_string());
        }

        let count = headers
            .get("lines")
            .ok_or(format_err!("lines header not provided"))
            .map(|v| v.parse::<usize>())??;

        let status = headers
            .get("status")
            .ok_or(format_err!("status header not provided"))
            .map(|v| v.parse::<Status>())??;

        let mut buffer = String::new();
        for _ in 0..count {
            read.read_line(&mut buffer)?;
        }
        buffer.pop(); //remove last new line char
        match status {
            Status::Ok => Ok(buffer),
            Status::Error => bail!("{}", buffer),
        }
    }
}

fn connect() -> Result<net::UnixStream> {
    let p = path::Path::new("/var/run").join(api::SOCKET_NAME);

    Ok(net::UnixStream::connect(p)?)
}

/// list command
pub fn list() -> Result<()> {
    let mut con = connect()?;
    let result = con.request("list")?;
    println!("{}", result);
    Ok(())
}

/// status command
pub fn status(name: &str) -> Result<()> {
    let mut con = connect()?;
    let result = con.request(&format!("status {}", name))?;
    println!("{}", result);
    Ok(())
}

/// stop command
pub fn stop(name: &str) -> Result<()> {
    let mut con = connect()?;
    let result = con.request(&format!("stop {}", name))?;
    println!("{}", result);
    Ok(())
}

/// start command
pub fn start(name: &str) -> Result<()> {
    let mut con = connect()?;
    let result = con.request(&format!("start {}", name))?;
    println!("{}", result);
    Ok(())
}

/// forget command
pub fn forget(name: &str) -> Result<()> {
    let mut con = connect()?;
    let result = con.request(&format!("forget {}", name))?;
    println!("{}", result);
    Ok(())
}

/// forget command
pub fn monitor(name: &str) -> Result<()> {
    let mut con = connect()?;
    let result = con.request(&format!("monitor {}", name))?;
    println!("{}", result);
    Ok(())
}

/// kill command
pub fn kill(name: &str, signal: &str) -> Result<()> {
    let mut con = connect()?;
    let result = con.request(&format!("kill {} {}", name, signal))?;
    println!("{}", result);
    Ok(())
}
