use failure::Error;
use nix::sys::wait::{self, WaitStatus};
use std::collections::HashMap;
use std::process::Command;
use std::sync::{Arc, Mutex};
use tokio::prelude::*;
use tokio::sync::oneshot::{self, Sender};
use tokio_signal::unix::Signal;

pub trait WaitStatusExt {
    fn success(&self) -> bool;
}

impl WaitStatusExt for WaitStatus {
    fn success(&self) -> bool {
        match *self {
            WaitStatus::Exited(_, code) if code == 0 => true,
            _ => false,
        }
    }
}

const SIGCHLD: i32 = 17;

type Result<T> = std::result::Result<T, Error>;

/// the process manager maintains a list or running processes
/// it also take care of reading exit status of processes when
/// they exit, by waiting on the SIGCHLD signal, and then make sure
/// to wake up the command future.
/// this allow us also to do orphan reaping for free.
pub struct ProcessManager {
    ps: Arc<Mutex<HashMap<u32, Sender<WaitStatus>>>>,
}

impl ProcessManager {
    pub fn new() -> ProcessManager {
        ProcessManager {
            ps: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn try_wait_process() -> Vec<WaitStatus> {
        let mut statuses: Vec<WaitStatus> = Vec::new();
        loop {
            let status = match wait::waitpid(Option::None, Some(wait::WaitPidFlag::WNOHANG)) {
                Ok(status) => status,
                Err(_) => {
                    return statuses;
                }
            };

            match status {
                WaitStatus::StillAlive => break,
                _ => statuses.push(status),
            }
        }

        statuses
    }

    /// creates a child process future from Command
    pub fn cmd(
        &mut self,
        cmd: &mut Command,
    ) -> Result<(u32, impl Future<Item = WaitStatus, Error = Error>)> {
        let child = cmd.spawn()?;
        let (sender, receiver) = oneshot::channel::<WaitStatus>();

        self.ps.lock().unwrap().insert(child.id(), sender);

        Ok((child.id(), receiver.map_err(|e| format_err!("{}", e))))
    }

    /// creates a child process future from command line
    pub fn child(
        &mut self,
        cmd: String,
    ) -> Result<(u32, impl Future<Item = WaitStatus, Error = Error>)> {
        let args = match shlex::split(&cmd) {
            Some(args) => args,
            _ => bail!("invalid command line"),
        };

        if args.len() < 1 {
            bail!("invalid command line");
        }

        let mut cmd = Command::new(&args[0]);
        let cmd = cmd.args(&args[1..]);

        self.cmd(cmd)
    }

    /// return the process manager future. it's up to the
    /// caller responsibility to make sure it is spawned
    pub fn run(&self) -> impl Future<Item = (), Error = ()> {
        let stream = Signal::new(SIGCHLD).flatten_stream();
        let ps = Arc::clone(&self.ps);

        stream.map_err(|_| ()).for_each(move |_signal| {
            let statuses = Self::try_wait_process();

            for status in statuses.into_iter() {
                let pid = match status.pid() {
                    Some(pid) => pid.as_raw() as u32,
                    None => {
                        //no pid, it means no child has exited
                        continue; //continue the loop
                    }
                };

                let mut ps = ps.lock().unwrap();
                let sender = match ps.remove(&pid) {
                    Some(sender) => sender,
                    None => continue,
                };
                match sender.send(status) {
                    Ok(_) => (),
                    Err(e) => println!("failed to notify child of pid '{}': {:?}", pid, e),
                };
            }

            Ok(())
        })
    }
}
