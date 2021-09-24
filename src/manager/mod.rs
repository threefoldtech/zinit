use std::collections::HashMap;

use anyhow::{Context, Result};
use nix::sys::signal;
use nix::sys::wait::{self, WaitStatus};
use nix::unistd::Pid;
use shlex;
use std::fs::File as StdFile;
use std::os::unix::io::FromRawFd;
use std::os::unix::io::IntoRawFd;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::signal::unix;
use tokio::sync::oneshot;
use tokio::sync::Mutex;

mod buffer;
pub use buffer::Logs;

pub struct Process {
    cmd: String,
    env: HashMap<String, String>,
    cwd: String,
}
type WaitChannel = oneshot::Receiver<WaitStatus>;

pub struct Child {
    pub pid: Pid,
    ch: WaitChannel,
}

impl Child {
    pub fn new(pid: Pid, ch: WaitChannel) -> Child {
        Child { pid: pid, ch: ch }
    }

    pub async fn wait(self) -> Result<WaitStatus> {
        Ok(self.ch.await?)
    }

    pub fn signal(&self, sig: signal::Signal) -> Result<()> {
        Ok(signal::kill(self.pid, sig)?)
    }
}

type Handler = oneshot::Sender<WaitStatus>;

impl Process {
    pub fn new<S: Into<String>>(cmd: S, cwd: S, env: Option<HashMap<String, String>>) -> Process {
        let env = match env {
            None => HashMap::new(),
            Some(env) => env,
        };

        Process {
            cmd: cmd.into(),
            env: env,
            cwd: cwd.into(),
        }
    }
}

#[derive(Clone)]
pub enum Log {
    None,
    Stdout,
    Ring(String),
}

#[derive(Clone)]
pub struct ProcessManager {
    table: Arc<Mutex<HashMap<Pid, Handler>>>,
    ring: buffer::Ring,
}

impl ProcessManager {
    pub fn new(cap: usize) -> ProcessManager {
        ProcessManager {
            table: Arc::new(Mutex::new(HashMap::new())),
            ring: buffer::Ring::new(cap),
        }
    }

    fn wait_process() -> Vec<WaitStatus> {
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

    pub fn start(&self) {
        let table = Arc::clone(&self.table);
        tokio::spawn(async move {
            let mut signals = match unix::signal(unix::SignalKind::child()) {
                Ok(s) => s,
                Err(err) => {
                    panic!("failed to bind to signals: {}", err);
                }
            };

            loop {
                signals.recv().await;
                let mut table = table.lock().await;
                for exited in Self::wait_process() {
                    if let Some(pid) = exited.pid() {
                        if let Some(sender) = table.remove(&pid) {
                            sender.send(exited).unwrap();
                        }
                    }
                }
            }
        });
    }

    fn sink(&self, file: File, prefix: String) {
        let ring = self.ring.clone();
        let reader = BufReader::new(file);

        tokio::spawn(async move {
            let mut lines = reader.lines();
            while let Ok(line) = lines.next_line().await {
                let _ = match line {
                    Some(line) => ring.push(format!("{}: {}", prefix, line)).await,
                    None => break,
                };
            }
        });
    }

    pub async fn stream(&self) -> Result<Logs> {
        self.ring.stream().await
    }

    pub fn signal(&self, pid: Pid, sig: signal::Signal) -> Result<()> {
        Ok(signal::kill(pid, sig)?)
    }

    pub async fn run(&self, cmd: Process, log: Log) -> Result<Child> {
        let args = shlex::split(&cmd.cmd).context("failed to parse command")?;
        if args.len() == 0 {
            bail!("invalid command");
        }

        let mut child = Command::new(&args[0]);

        let child = if cmd.cwd.len() > 0 {
            child.current_dir(&cmd.cwd)
        } else {
            child.current_dir("/")
        };

        let child = child.args(&args[1..]).envs(cmd.env);

        let child = match log {
            Log::None => child.stdin(Stdio::null()).stdout(Stdio::null()),
            Log::Ring(_) => child.stdout(Stdio::piped()).stderr(Stdio::piped()),
            _ => child, // default to inherit
        };

        let mut child = child.spawn().context("failed to spawn command")?;

        if let Log::Ring(prefix) = log {
            if let Some(out) = child.stdout.take() {
                let out = File::from_std(unsafe { StdFile::from_raw_fd(out.into_raw_fd()) });
                self.sink(out, format!("[+] {}", prefix))
            }

            if let Some(out) = child.stderr.take() {
                let out = File::from_std(unsafe { StdFile::from_raw_fd(out.into_raw_fd()) });
                self.sink(out, format!("[+] {}", prefix))
            }
        }

        let (tx, rx) = oneshot::channel();

        let id = child.id();

        let mut table = self.table.lock().await;
        let pid = Pid::from_raw(id as i32);
        table.insert(pid, tx);

        Ok(Child::new(pid, rx))
    }
}
