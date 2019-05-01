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
/// State is the process state
#[derive(Debug)]
enum State {
    Scheduled,
    Running,
    Success,
    Error(ExitStatus),
    Failure,
}

#[derive(Debug)]
enum Message {
    Monitor(String, Service),
    Exit(String, ExitStatus),
    ReSpawn(String),
}

pub struct Handle {
    tx: Sender,
}

impl Handle {
    pub fn monitor(&self, name: String, service: Service) {
        //self.states.insert(name.clone(), State::Scheduled);
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
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        Manager {
            processes: HashMap::new(),
            tx: tx,
            rx: Some(rx),
        }
    }

    fn exec(&mut self, name: String, config: Service) {
        let child = match Command::new("date").spawn_async() {
            Ok(child) => child,
            Err(err) => {
                self.processes.get_mut(&name).unwrap().state = State::Failure;
                return;
            }
        };

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
        let config = service.clone();
        self.processes.insert(name.clone(), Process::new(service));
        self.exec(name.clone(), config);
    }

    /// handle the re-spawn message
    fn re_spawn(&mut self, name: String) {
        let process = self.processes.get(&name).unwrap();
        let config = process.config.clone();
        self.exec(name.clone(), config);
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

    fn process(&mut self, msg: Message) {
        match msg {
            Message::Monitor(name, service) => self.monitor(name, service),
            Message::Exit(name, status) => self.exit(name, status),
            Message::ReSpawn(name) => self.re_spawn(name),
            _ => println!("Unhandled message {:?}", msg),
        }
    }

    pub fn run(mut self) -> (Handle, impl Future<Item = (), Error = ()>) {
        println!("running manager");

        let rx = match self.rx.take() {
            Some(rx) => rx,
            None => panic!("manager is already running"),
        };

        let tx = self.tx.clone();

        let future = rx
            .for_each(move |msg| {
                self.process(msg);
                Ok(())
            })
            .map_err(|e| {
                println!("error: {}", e);
                ()
            });

        return (Handle { tx }, future);
    }
}
