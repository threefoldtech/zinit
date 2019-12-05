use crate::ring::RingLog;
use failure::Error;
use nix::sys::signal;
use nix::sys::wait::WaitStatus;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::u64::MAX;
use tokio::prelude::*;
use tokio::sync::mpsc;
use tokio::timer;

use crate::settings::Service;

mod pm;
use pm::WaitStatusExt;

type Result<T> = std::result::Result<T, Error>;

/// Process is a representation of a scheduled/running
/// service
#[derive(Clone)]
pub struct Process {
    pub pid: u32,
    pub state: State,
    // config is the service configuration
    pub config: Service,
    // target is the target state of the service (up, down)
    pub target: Target,
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

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Up,
    Down,
}
/// Service state
#[derive(Debug, PartialEq, Clone)]
pub enum State {
    /// service is unknown state or service is not defined
    Unknown,
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
    //Monitor(String, Service),
    /// Exit, notify the manager that a service has exited with the given
    /// exit status. Once the manager receives this message, it decides
    /// either to re-spawn or exit based on the service configuration provided
    /// by the monitor message
    Exit(String, WaitStatus),
    /// State message sets the service state to the given value
    State(String, State),
    /// Spawn a service after exit
    Spawn(String),
}

pub struct Handle {
    //tx: MessageSender,
    inner: Arc<Mutex<Manager>>,
}

impl Clone for Handle {
    fn clone(&self) -> Self {
        Handle {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Handle {
    pub fn monitor(&self, name: String, service: Service) -> Result<()> {
        self.inner.lock().unwrap().monitor(name, service)
    }

    pub fn list(&self) -> Vec<String> {
        self.inner.lock().unwrap().list()
    }

    pub fn stop<T: AsRef<str>>(&self, name: T) -> Result<()> {
        self.inner.lock().unwrap().stop(name.as_ref())
    }

    pub fn forget<T: AsRef<str>>(&self, name: T) -> Result<()> {
        self.inner.lock().unwrap().forget(name.as_ref())
    }

    pub fn kill<T: AsRef<str>>(&self, name: T, signal: signal::Signal) -> Result<()> {
        self.inner.lock().unwrap().kill(name.as_ref(), signal)
    }

    pub fn start<T: AsRef<str>>(&self, name: T) -> Result<()> {
        self.inner.lock().unwrap().start(name.as_ref())
    }

    pub fn status<T: AsRef<str>>(&self, name: T) -> Result<Process> {
        self.inner.lock().unwrap().status(name.as_ref())
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
    testers: Arc<Mutex<HashMap<String, ()>>>,
}

impl Manager {
    /// creates a new manager instance
    pub fn new(log: Arc<RingLog>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        Manager {
            pm: Arc::new(Mutex::new(pm::ProcessManager::new(log))),
            processes: HashMap::new(),
            tx: tx,
            rx: Some(rx),
            testers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// tester will create a tester future that tries to run the given test command
    /// with delays 250*trial number ms in between.
    /// so the delay is 0 250 500 750 1000 1250 for a max delay of 2 seconds.
    /// The test will keep retrying until the test succeed, or the service is marked
    /// as Down.
    fn tester(&mut self, id: String, cmd: Option<Service>) -> impl Future<Item = (), Error = ()> {
        use std::time::{Duration, Instant};
        let cmd = match cmd {
            Some(cmd) => cmd,
            None => {
                return future::Either::A(future::ok(()));
            }
        };

        let table = Arc::clone(&self.pm);
        let testers = Arc::clone(&self.testers);
        let tester = stream::iter_ok(0..MAX) //try almost forever before giving up
            .for_each(move |i| {
                let id = id.clone();
                let testers = Arc::clone(&testers);
                if let None = testers.lock().unwrap().get(&id) {
                    // the test is not register anymore
                    // we need to break the loop
                    return future::Either::B(future::err(()));
                }

                let cmd = cmd.clone();
                let table = Arc::clone(&table);

                let mut ms = i * 250;
                if ms > 2000 {
                    // max wait of 2 seconds between tests.
                    ms = 2000;
                }
                let deadline = Instant::now() + Duration::from_millis(ms);
                let delayed = timer::Delay::new(deadline)
                    .map_err(|e| format_err!("{}", e))
                    .and_then(move |_| {
                        match table.lock().unwrap().child(format!("{}/test", id), cmd) {
                            Ok((_, child)) => future::Either::A(child),
                            Err(err) => future::Either::B(future::err(err)),
                        }
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
                    });

                future::Either::A(delayed)
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
        let config = process.config.clone();
        let test = config.test_as_service();
        drop(process);

        let service = name.clone();
        let child = match self.pm.lock().unwrap().child(name.clone(), config) {
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
        let service = name.clone();
        let testers = Arc::clone(&self.testers);
        let tester = self
            .tester(name.clone(), test)
            .then(move |result| {
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
                // clear the testers map
                testers.lock().unwrap().remove(&service);
                tx.send(Message::State(service, state))
            })
            .map(|_| ())
            .map_err(|_| ());

        // since testers now never exit until they succeed, waiting a max of 2 seconds
        // between testing. We need to make sure if a daemon fail, (while it's tester loop never exited)
        // that we don't start another tester loop.
        // hence, we make sure we mark running tests in the testers map. the map also works as a flag
        // for the test loop itself, if the loop can't find a flag for it's id, it will exit.
        let mut testers = self.testers.lock().unwrap();
        match testers.get(&name) {
            Some(_) => (),
            None => {
                testers.insert(name, ());
                tokio::spawn(tester);
            }
        }
    }

    fn can_schedule(&self, service: &Service) -> bool {
        let mut can = true;
        for dep in service.after.iter() {
            can = match self.processes.get(dep) {
                Some(ps) => match ps.state {
                    State::Running if !ps.config.one_shot => true,
                    State::Success => true,
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

    fn spawn(&mut self, name: String) {
        let tx = self.tx.clone();
        tokio::spawn(
            tx.send(Message::Spawn(name))
                .map(|_| ())
                .map_err(|e| error!("failed to spawn service: {}", e)),
        );
    }

    /// handle the monitor message
    fn monitor(&mut self, name: String, service: Service) -> Result<()> {
        service.validate()?;

        if self.processes.contains_key(&name) {
            bail!("service with name '{}' already monitored", name);
        }

        let can_schedule = self.can_schedule(&service);

        let state = match can_schedule {
            true => State::Scheduled,
            false => State::Blocked,
        };

        self.processes
            .insert(name.clone(), Process::new(service, state));

        // we can start this service immediately
        if can_schedule {
            self.spawn(name);
        }

        Ok(())
    }

    /// handle the re-spawn message
    fn os_spawn(&mut self, name: String) {
        // the re-spawn message will check the target
        // service state, and only actually re-spawn if the target
        // is up.

        let process = match self.processes.get(&name) {
            Some(process) => process,
            None => return,
        };

        match process.state {
            State::Running | State::Spawned => return,
            _ => match process.target {
                Target::Up => self.exec(name),
                Target::Down => return,
            },
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
            self.spawn(name);
        }
    }

    /// handle process exit
    fn on_exit(&mut self, name: String, status: WaitStatus) {
        let process = match self.processes.get_mut(&name) {
            Some(process) => process,
            None => return,
        };

        process.pid = 0;
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

        if process.config.one_shot {
            // OneShot service is the only one that can
            // be set to either success or error.
            // the reason that we do it async is that we
            // can have a single entry point for service
            // status update, so it's either to add hooks
            // to the state change event.
            return;
        }

        use std::time::{Duration, Instant};

        let tx = self.tx.clone();
        let now = Instant::now() + Duration::from_secs(1);
        let f = timer::Delay::new(now)
            .map_err(|_| panic!("timer failed"))
            .and_then(move |_| tx.send(Message::Spawn(name)))
            .map(|_| ())
            .map_err(|_| ());

        tokio::spawn(f);
    }

    /// stop action.
    fn stop(&mut self, name: &str) -> Result<()> {
        // stop a service by name
        // 1- we need to set the required target of a service
        // 2- make sure no running testers for this service
        // 3- we need to signal the service to stop
        let process = match self.processes.get_mut(name) {
            Some(process) => process,
            None => {
                bail!("unknown service name {}", name);
            }
        };

        // make sure no tests are running for this service
        self.testers.lock().unwrap().remove(name);

        process.target = Target::Down;
        if process.pid <= 0 {
            return Ok(());
        }

        use nix::unistd::Pid;
        use std::str::FromStr;
        let sig = signal::Signal::from_str(&process.config.signal.stop.to_uppercase())?;

        signal::kill(Pid::from_raw(process.pid as i32), sig)?;

        Ok(())
    }

    /// kill action. send signal to any service
    fn kill(&mut self, name: &str, signal: signal::Signal) -> Result<()> {
        let process = match self.processes.get_mut(name) {
            Some(process) => process,
            None => {
                bail!("unknown service name {}", name);
            }
        };

        if process.pid <= 0 {
            bail!("service is not running");
        }

        use nix::unistd::Pid;

        signal::kill(Pid::from_raw(process.pid as i32), signal)?;

        Ok(())
    }

    /// start action
    fn start(&mut self, name: &str) -> Result<()> {
        // this action has no way to communicate
        // the error back to the caller yet.

        // start a service by name
        // 1- we need to set the required state of a service
        // 2- we need to spawn the service if it's not already running
        let process = match self.processes.get_mut(name) {
            Some(process) => process,
            None => {
                bail!("unknown service name {}", name);
            }
        };

        process.target = Target::Up;
        self.spawn(String::from(name));

        Ok(())
    }

    /// status action, reads current service status
    fn status(&mut self, name: &str) -> Result<Process> {
        // this action has no way to communicate
        // the error back to the caller yet.

        // start a service by name
        // 1- we need to set the required state of a service
        // 2- we need to exec the service if it's not already running
        let process = match self.processes.get_mut(name) {
            Some(process) => process,
            None => {
                bail!("unknown service name {}", name);
            }
        };

        Ok(process.clone())
    }

    /// list action
    fn list(&self) -> Vec<String> {
        // this action has no way to communicate
        // the error back to the caller yet.
        let mut services = vec![];
        for key in self.processes.keys() {
            services.push(key.clone());
        }

        services
    }

    fn forget(&mut self, name: &str) -> Result<()> {
        let process = match self.processes.get_mut(name) {
            Some(process) => process,
            None => {
                bail!("unknown service name {}", name);
            }
        };

        if process.target != Target::Down {
            bail!("service target must be set to `Down`");
        }

        if process.state == State::Running || process.state == State::Spawned || process.pid > 0 {
            bail!("service is running");
        }

        self.processes.remove(name);

        Ok(())
    }

    /// process different message types
    fn process(&mut self, msg: Message) {
        match msg {
            Message::Exit(name, status) => self.on_exit(name, status),
            Message::Spawn(name) => self.os_spawn(name),
            Message::State(name, state) => self.on_state(name, state),
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

        let mgr = Arc::new(Mutex::new(self));
        // start the process manager main loop
        let inner = Arc::clone(&mgr);
        let future = rx
            .for_each(move |msg| {
                mgr.lock().unwrap().process(msg);
                Ok(())
            })
            .map_err(|e| {
                println!("error: {}", e);
                ()
            });

        tokio::spawn(future);
        Handle { inner }
    }
}
