use failure::Error;
use futures;
use futures::lazy;
use nix::sys::wait::WaitStatus;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::prelude::*;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::timer;

use crate::settings::Service;

mod pm;
use pm::WaitStatusExt;

type Result<T> = std::result::Result<T, Error>;

/// Process is a representation of a scheduled/running
/// service
struct Process {
    pid: u32,
    state: State,
    // config is the service configuration
    config: Service,
    // target is the target state of the service (up, down)
    target: Target,
}

impl Process {
    fn new(service: Service, state: State) -> Process {
        Process {
            pid: 0,
            state: state,
            config: service,
            target: Target::Up,
        }
    }
}

enum Target {
    Up,
    Down,
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

#[derive(Debug)]
enum Action {
    Monitor { name: String, service: Service },
    Stop { name: String },
}
/// Message defines the process manager internal messaging
#[derive(Debug)]
enum Message {
    /// Monitor, starts a new service and monitor it
    //Monitor(String, Service),
    /// Exit, notify the manager that a service has exited with the given
    /// exit status. Once the manager receives this message, it decides
    /// either to re-spawn or exit based on the service configuration provided
    /// by the monitor message
    Exit(String, WaitStatus),
    /// State message sets the service state to the given value
    State(String, State),
    /// ReSpawn a service after exit
    ReSpawn(String),

    Action(Action),
}

pub struct Handle {
    tx: MessageSender,
}

impl Handle {
    pub fn monitor(&self, name: String, service: Service) {
        let tx = self.tx.clone();
        tokio::spawn(lazy(move || {
            tx.send(Message::Action(Action::Monitor {
                name: name,
                service: service,
            }))
            .map(|_| ())
            .map_err(|_| ())
        }));
    }

    pub fn stop(&self, name: String) {
        let tx = self.tx.clone();
        tokio::spawn(lazy(move || {
            tx.send(Message::Action(Action::Stop { name: name }))
                .map(|_| ())
                .map_err(|_| ())
        }));
    }

    pub fn stop2(&self, name: String) -> impl Future<Item = Result<()>, Error = Error> {
        let tx = self.tx.clone();
        let (sender, receiver) = oneshot::channel::<Result<()>>();

        tx.send(Message::Action(Action::Stop { name: name }))
            .map_err(|e| format_err!("{}", e))
            .and_then(move |_| receiver.map_err(|e| format_err!("{}", e)))
    }
}

type MessageSender = mpsc::UnboundedSender<Message>;
type MessageReceiver = mpsc::UnboundedReceiver<Message>;

/// Manager is the main entry point, it keeps track of the
/// processes state, and spawn them based on the dependencies.
pub struct Manager {
    pm: Arc<Mutex<pm::ProcessManager>>,
    processes: HashMap<String, Process>,
    tx: MessageSender,
    rx: Option<MessageReceiver>,
}

impl Manager {
    /// creates a new manager instance
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        Manager {
            pm: Arc::new(Mutex::new(pm::ProcessManager::new())),
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

        let table = Arc::clone(&self.pm);
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
        let child = match self.pm.lock().unwrap().child(exec) {
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
    fn on_re_spawn(&mut self, name: String) {
        // the re-spawn message will check the target
        // service state, and only actually re-spawn if the target
        // is up.

        let process = match self.processes.get(&name) {
            Some(process) => process,
            None => return,
        };

        match process.target {
            Target::Up => self.exec(name),
            Target::Down => return,
        }
    }

    /// set service state
    fn on_state(&mut self, name: String, state: State) {
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
    fn on_exit(&mut self, name: String, status: WaitStatus) {
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

    fn stop(&mut self, name: String) {
        // this action has no way to communicate
        // the error back to the caller yet.

        // stop a service by name
        // 1- we need to set the required state of a service
        // 2- we need to signal the service to stop
        let process = match self.processes.get_mut(&name) {
            Some(process) => process,
            None => return,
        };

        process.target = Target::Down;
        use nix::sys::signal;
        use nix::unistd::Pid;

        // todo:
        // currently we only signal kill with sig-term, later on we need to
        // allow service configuration its own signals for different operations (stop, reload)
        signal::kill(Pid::from_raw(process.pid as i32), signal::Signal::SIGTERM);
    }

    fn on_action(&mut self, action: Action) {
        match action {
            Action::Monitor { name, service } => self.monitor(name, service),
            Action::Stop { name } => self.stop(name),
        }
    }

    /// process different message types
    fn process(&mut self, msg: Message) {
        match msg {
            Message::Exit(name, status) => self.on_exit(name, status),
            Message::ReSpawn(name) => self.on_re_spawn(name),
            Message::State(name, state) => self.on_state(name, state),
            Message::Action(action) => self.on_action(action),
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
        tokio::spawn(self.pm.lock().unwrap().run());

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
