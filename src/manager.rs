use failure::Error;
use futures::{future, lazy};
use std::collections::HashMap;
use std::process::{Command, ExitStatus};
use tokio::prelude::*;
use tokio::sync::mpsc;
use tokio::timer;
use tokio_process::CommandExt;

use crate::settings::Service;

struct Process {
    state: State,
    config: Service,
}

impl Process {
    fn new(service: Service) -> Process {
        return Process {
            state: State::Scheduled,
            config: service,
        };
    }
}
/// Service state
#[derive(Debug)]
enum State {
    /// service is scheduled for execution
    Scheduled,
    /// service has been started, it didn't exit yet (or status wasn't checked until this moment)
    Running,
    /// service has exited with success state, only one-shot can stay in this state
    Success,
    /// service exited with this error
    Error(ExitStatus),
    /// failed to spawn the process (todo: add more info)
    Failure,
}

/// Message defines the process manager internal messaging
#[derive(Debug)]
enum Message {
    /// Monitor, starts a new service and monitor it
    Monitor(String, Service),
    /// Exit, notify the manager that a service has exited with the given
    /// exit status. Once the manager receives this message, it decides
    /// either to re-spawn or exit based on the service configuration provided
    /// by the monitor message
    Exit(String, ExitStatus),
    /// ReSpawn a service after exit
    ReSpawn(String),
}

pub struct Handle {
    tx: Sender,
}

impl Handle {
    pub fn monitor(&self, name: String, service: Service) {
        let tx = self.tx.clone();
        tokio::spawn(lazy(move || {
            tx.send(Message::Monitor(name, service))
                .map(|_| ())
                .map_err(|_| ())
        }));
    }
}

type Sender = mpsc::UnboundedSender<Message>;

/// Manager is the main entry point, it keeps track of the
/// processes state, and spawn them based on the dependencies.
pub struct Manager {
    processes: HashMap<String, Process>,
    tx: Sender,
    rx: Option<mpsc::UnboundedReceiver<Message>>,
}

impl Manager {
    /// creates a new manager instance
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        Manager {
            processes: HashMap::new(),
            tx: tx,
            rx: Some(rx),
        }
    }

    /// exec a service given the name
    fn exec(&mut self, name: String) {
        let mut process = self.processes.get_mut(&name).unwrap();

        let args = shlex::split(&process.config.exec).unwrap();
        let child = match Command::new(&args[0]).args(&args[1..]).spawn_async() {
            Ok(child) => child,
            Err(err) => {
                process.state = State::Failure;
                return;
            }
        };

        process.state = State::Running;
        //TODO: for long running services the `test` command line
        //must be executed after spawning the child
        let tx = self.tx.clone();
        let future = child
            .map_err(|_| {
                panic!("failed to wait for child");
            })
            .and_then(move |status| tx.send(Message::Exit(name, status)))
            .map(|_| ())
            .map_err(|_| ());

        tokio::spawn(future);
    }

    /// handle the monitor message
    fn monitor(&mut self, name: String, service: Service) {
        self.processes.insert(name.clone(), Process::new(service));
        self.exec(name);
    }

    /// handle the re-spawn message
    fn re_spawn(&mut self, name: String) {
        self.exec(name);
    }

    /// handle process exit
    fn exit(&mut self, name: String, status: ExitStatus) {
        let process = match self.processes.get_mut(&name) {
            Some(process) => process,
            None => return,
        };

        process.state = if status.success() {
            State::Success
        } else {
            State::Error(status)
        };

        if process.config.one_shot {
            return;
        }

        use std::time::{Duration, Instant};

        let tx = self.tx.clone();
        let now = Instant::now() + Duration::from_secs(2);
        let f = timer::Delay::new(now)
            .map_err(|_| panic!("timer failed"))
            .and_then(move |_| tx.send(Message::ReSpawn(name)))
            .map(|_| ())
            .map_err(|_| ());

        tokio::spawn(f);
    }
    /// process different message types
    fn process(&mut self, msg: Message) {
        match msg {
            Message::Monitor(name, service) => self.monitor(name, service),
            Message::Exit(name, status) => self.exit(name, status),
            Message::ReSpawn(name) => self.re_spawn(name),
            _ => println!("Unhandled message {:?}", msg),
        }
    }

    /// run moves the manager, and return a handle.
    pub fn run(mut self) -> Handle {
        let rx = match self.rx.take() {
            Some(rx) => rx,
            None => panic!("manager is already running"),
        };

        let tx = self.tx.clone();

        // start the process manager main loop
        let future = rx
            .for_each(move |msg| {
                self.process(msg);
                Ok(())
            })
            .map_err(|e| {
                println!("error: {}", e);
                ()
            });

        tokio::spawn(future);
        Handle { tx }
    }
}
