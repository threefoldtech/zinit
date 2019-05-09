use failure::Error;
use futures;
use futures::lazy;
use nix::sys::wait::{self, WaitStatus};
use std::collections::HashMap;
use std::process::Command;
use std::sync::{Arc, Mutex};
use tokio::prelude::*;
use tokio::sync::mpsc;
use tokio::sync::oneshot::{self, Sender};
use tokio::timer;
use tokio_signal::unix::Signal;

use crate::settings::Service;

trait WaitStatusExt {
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
struct ProcessManager {
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

/// Process is a representation of a scheduled/running
/// service
struct Process {
    pid: u32,
    state: State,
    config: Service,
}

impl Process {
    fn new(service: Service, state: State) -> Process {
        Process {
            pid: 0,
            state: state,
            config: service,
        }
    }
}
/// Service state
#[derive(Debug, PartialEq)]
enum State {
    /// Blocked means one or more dependencies hasn't been met yet. Service can stay in
    /// this state as long as at least one dependency is not in either Running, or Success
    Blocked,
    /// service is scheduled for execution
    Scheduled,
    /// service has been started, but it didn't exit yet, or we didn't run the test command.
    Spawned,
    /// service has been started, and test command passed.
    Running,
    /// service has exited with success state, only one-shot can stay in this state
    Success,
    /// service exited with this error, only one-shot can stay in this state
    Error(WaitStatus),
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
    Exit(String, WaitStatus),
    /// State message sets the service state to the given value
    State(String, State),
    /// ReSpawn a service after exit
    ReSpawn(String),
}

pub struct Handle {
    tx: UBSender,
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

type UBSender = mpsc::UnboundedSender<Message>;

/// Manager is the main entry point, it keeps track of the
/// processes state, and spawn them based on the dependencies.
pub struct Manager {
    table: Arc<Mutex<ProcessManager>>,
    processes: HashMap<String, Process>,
    tx: UBSender,
    rx: Option<mpsc::UnboundedReceiver<Message>>,
}

impl Manager {
    /// creates a new manager instance
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        Manager {
            table: Arc::new(Mutex::new(ProcessManager::new())),
            processes: HashMap::new(),
            tx: tx,
            rx: Some(rx),
        }
    }

    /// tester will create a tester future that tries to run the given command 5 times
    /// with delays 250*trial number ms in between.
    /// so the delay is 0 250 500 750 1000 1250 for a total delay of 3750 ms before
    /// the service is set to "test failed" state.
    fn tester(&mut self, cmd: String) -> impl Future<Item = (), Error = ()> {
        use std::time::{Duration, Instant};
        if cmd.is_empty() {
            return future::Either::A(future::ok(()));
        }

        let table = Arc::clone(&self.table);
        let tester = stream::iter_ok(0..5) //try 5 times (configurable ?)
            .for_each(move |i| {
                let cmd = cmd.clone();
                let table = Arc::clone(&table);
                // wait 250 ms between trials (configurable ?)
                let deadline = Instant::now() + Duration::from_millis(i * 250);
                timer::Delay::new(deadline)
                    .map_err(|e| format_err!("{}", e))
                    .and_then(move |_| match table.lock().unwrap().child(cmd) {
                        Ok((_, child)) => future::Either::A(child),
                        Err(err) => future::Either::B(future::err(err)),
                    })
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

        let tx = self.tx.clone();
        process.state = State::Spawned;
        let exec = process.config.exec.clone();
        let test = process.config.test.clone();
        drop(process);

        let service = name.clone();
        let child = match self.table.lock().unwrap().child(exec) {
            Ok((pid, child)) => {
                // update the process pid
                let mut process = self.processes.get_mut(&name).unwrap();
                process.pid = pid;
                child
            }
            Err(_) => {
                // failed to spawn child, this is probably an
                // un-fixable error. we set status to failure and exit
                // todo: add error to the failure
                tokio::spawn(
                    tx.send(Message::State(service, State::Failure))
                        .map(|_| ())
                        .map_err(|_| ()),
                );
                return;
            }
        };

        let service = name.clone();
        let child = child
            .and_then(move |status| {
                // once child exits, we send the status to process table
                tx.send(Message::Exit(service, status))
                    .map_err(|e| format_err!("{}", e))
            })
            .map(|_| ())
            .map_err(|e| {
                println!("failed exec process: {}", e);
            });

        tokio::spawn(child);

        let tx = self.tx.clone();
        let tester = self
            .tester(test)
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

    fn can_schedule(&self, service: &Service) -> bool {
        let mut can = true;
        for dep in service.after.iter() {
            can = match self.processes.get(dep) {
                Some(ps) => match ps.state {
                    State::Running | State::Success => true,
                    _ => false,
                },
                //depending on an undefined service. This still can be resolved later
                //by monitoring the dependency in the future.
                None => false,
            };

            // if state is blocked, we can break the loop
            if !can {
                break;
            }
        }

        can
    }

    /// handle the monitor message
    fn monitor(&mut self, name: String, service: Service) {
        let can_schedule = self.can_schedule(&service);

        let state = match can_schedule {
            true => State::Scheduled,
            false => State::Blocked,
        };

        self.processes
            .insert(name.clone(), Process::new(service, state));

        // we can start this service immediately
        if can_schedule {
            self.exec(name);
        }
    }

    /// handle the re-spawn message
    fn re_spawn(&mut self, name: String) {
        self.exec(name);
    }

    /// set service state
    fn state(&mut self, name: String, state: State) {
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

        // set process state
        process.state = state;

        // take an action based on the service state.
        match process.state {
            State::Running | State::Success => self.unblock(),
            _ => {}
        };
    }

    /// unblock scans the full list of services to find any blocked
    /// service that can be unblocked and schedule it.
    fn unblock(&mut self) {
        let mut to_schedule = vec![];
        for (name, ps) in self.processes.iter() {
            if ps.state == State::Blocked && self.can_schedule(&ps.config) {
                to_schedule.push(name.clone());
            }
        }

        for name in to_schedule {
            self.exec(name);
        }
    }

    /// handle process exit
    fn exit(&mut self, name: String, status: WaitStatus) {
        let process = match self.processes.get_mut(&name) {
            Some(process) => process,
            None => return,
        };

        process.pid = 0;
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
        let now = Instant::now() + Duration::from_secs(1);
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

        //start the process table
        tokio::spawn(self.table.lock().unwrap().run());

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
