use failure::Error;
use futures::lazy;
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
#[derive(Debug, PartialEq)]
enum State {
    /// service is scheduled for execution
    Scheduled,
    /// service has been started, but it didn't exit yet, or we didn't run the test command.
    Spawned,
    /// service has been started, and test command passed.
    Running,
    /// service has exited with success state, only one-shot can stay in this state
    Success,
    /// service exited with this error
    Error(ExitStatus),
    /// the service test command failed, this might (or might not) be replaced
    /// with an Error state later on once the service process itself exits
    TestError,
    /// Failure means the service has failed to spawn in a way that retyring
    /// won't help, like command line parsing error or failed to fork
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
    /// State message sets the service state to the given value
    State(String, State),
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

    /// creates a child process future
    fn child(cmd: String) -> impl Future<Item = ExitStatus, Error = std::io::Error> {
        let args = match shlex::split(&cmd) {
            Some(args) => args,
            _ => {
                return future::Either::B(future::err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "invalid command line",
                )))
            }
        };

        if args.len() < 1 {
            return future::Either::B(future::err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid command line",
            )));
        }

        let child = match Command::new(&args[0]).args(&args[1..]).spawn_async() {
            Ok(child) => child,
            Err(err) => return future::Either::B(future::err(err)),
        };

        future::Either::A(child)
    }

    /// tester will create a tester future that tries to run the given command 5 times
    /// with delays 250*trial number ms in between.
    /// so the delay is 0 250 500 750 1000 1250 for a total delay of 3750 ms before
    /// the service is set to "test failed" state.
    fn tester(cmd: String) -> impl Future<Item = (), Error = ()> {
        use std::time::{Duration, Instant};
        if cmd.is_empty() {
            return future::Either::A(future::ok(()));
        }

        let tester = stream::iter_ok(0..5) //try 5 times (configurable ?)
            .for_each(move |i| {
                let cmd = cmd.clone();
                // wait 250 ms between trials (configurable ?)
                let deadline = Instant::now() + Duration::from_millis(i * 250);
                timer::Delay::new(deadline)
                    .then(move |_| Self::child(cmd))
                    .then(|exit| {
                        if match exit {
                            Ok(status) => status.success(),
                            Err(_) => false,
                        } {
                            // On test command success we return an error
                            // to stop the foreach loop.
                            return Err(()); //break the loop
                        }

                        // returning an OK will continue the loop
                        // which means try again, so we return this
                        // if the test command fails!
                        Ok(())
                    })
            })
            // this confusing block will flip the result of this
            // future ok => err and err => ok the reason we do
            // this we want this future to return OK when the
            // test cmd success. But we actually break the loop with Err
            // if the future run to success it means we reached the end of the
            // loop without the test command runs to success, we need to convert
            // Ok to error to donate failure of test.
            .then(|r| match r {
                Ok(_) => Err(()),
                Err(_) => Ok(()),
            });

        future::Either::B(tester)
    }

    /// exec a service given the name
    fn exec(&mut self, name: String) {
        let mut process = self.processes.get_mut(&name).unwrap();
        let child = Self::child(process.config.exec.clone());

        process.state = State::Spawned;
        //TODO: for long running services the `test` command line
        //must be executed after spawning the child
        let tx = self.tx.clone();

        let service = name.clone();
        let child = child
            .then(move |result| match result {
                Ok(status) => tx.send(Message::Exit(service, status)),
                Err(_) => tx.send(Message::State(service, State::Failure)),
            })
            .map(|_| ())
            .map_err(|_| ());

        tokio::spawn(child);

        let tx = self.tx.clone();
        let tester = Self::tester(process.config.test.clone())
            .then(|result| {
                // the tester is the only entity that can change service
                // state to Running, or TestError
                // it means that we can safely assume the Running
                // or TestError state can only be set if the service
                // current state is set to `spawned`. This way we can avoid
                // state race conditions (for example if the service
                // exited before the test.
                let state = match result {
                    Ok(_) => State::Running,
                    Err(_) => State::TestError,
                };

                tx.send(Message::State(name, state))
            })
            .map(|_| ())
            .map_err(|_| ());

        tokio::spawn(tester);
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

    /// set service state
    fn state(&mut self, name: String, state: State) {
        //TODO: add reason of failure
        let process = match self.processes.get_mut(&name) {
            Some(process) => process,
            None => return,
        };

        // as explained before a service state can be only set
        // to running or test-error if the service state is
        // set to spawned. this is to avoid race conditions
        // in case the service itself changed state to success
        // or error (only possible from a one-shot service)
        let state = match state {
            State::Running | State::TestError if process.state != State::Spawned => return,
            _ => state,
        };

        process.state = state;
    }

    /// handle process exit
    fn exit(&mut self, name: String, status: ExitStatus) {
        let process = match self.processes.get_mut(&name) {
            Some(process) => process,
            None => return,
        };

        if process.config.one_shot {
            // OneShot service is the only one that can
            // be set to either success or error.
            // the reason that we do it async is that we
            // can have a single entry point for service
            // status update, so it's either to add hooks
            // to the state change event.
            let state = match status.success() {
                true => State::Success,
                false => State::Error(status),
            };
            let tx = self.tx.clone();
            tokio::spawn(
                tx.send(Message::State(name.clone(), state))
                    .map(|_| ())
                    .map_err(|_| ()),
            );
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
            Message::State(name, state) => self.state(name, state),
            //_ => println!("Unhandled message {:?}", msg),
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
