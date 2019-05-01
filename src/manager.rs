use failure::Error;
use futures::{future, lazy};
use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;
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
    Error { code: u16 },
}

#[derive(Debug)]
enum Message {
    Tick,
    State(State),
    Monitor((String, Service)),
    Exit,
}

pub struct Handle {
    tx: Sender,
}

impl Handle {
    pub fn monitor(&self, name: String, service: Service) {
        //self.states.insert(name.clone(), State::Scheduled);
        let tx = self.tx.clone();
        tokio::spawn(lazy(move || {
            tx.send(Message::Monitor((name, service)))
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

    fn keep_alive(&self) {
        // this ticker is just to make sure the tokio runtime will never shutdown
        // even if no more services are being monitored.
        // By having a long living future that will never exit
        tokio::spawn(lazy(|| {
            let ticker = timer::Interval::new_interval(Duration::from_secs(600));
            ticker
                .for_each(move |t| {
                    Ok(())
                    // let tx = tx.clone();
                    // tx.send(Message::Tick).then(|_| Ok(()))
                })
                .map(|_| ())
                .map_err(|_| ())
        }));
    }

    fn monitor(&mut self, name: String, service: Service) {
        self.processes.insert(name.clone(), Process::new(service));
        exec(self.tx.clone(), name);
        println!("executed");
    }

    fn process(&mut self, msg: Message) {
        match msg {
            Message::Monitor((name, service)) => self.monitor(name, service),
            _ => println!("Unhandled message {:?}", msg),
        }
    }

    pub fn run(mut self) -> (Handle, impl Future<Item = (), Error = ()>) {
        println!("running manager");

        let rx = match self.rx.take() {
            Some(rx) => rx,
            None => panic!("manager is already running"),
        };

        //self.keep_alive();
        let tx = self.tx.clone();

        let future = rx
            .for_each(move |msg| {
                println!("got message {:?}", msg);
                self.process(msg);
                //println!("message");
                Ok(())
            })
            //.map(|_| println!("exit loop"))
            .map_err(|e| {
                println!("error: {}", e);
                ()
            });

        return (Handle { tx }, future);
    }
}

fn exec(tx: Sender, cmd: String) -> Result<(), Error> {
    let child = Command::new("echo")
        .arg("hello")
        .arg("world")
        .spawn_async()?;

    let future = child
        .map(move |status| {
            println!("exit status {}", status);
            tx.send(Message::Exit).and_then(|tx| tx.flush())
        })
        .map(|tx| println!("transmitted"))
        .map_err(|e| {
            println!("failed to wait for exit status: {}", e);
        });

    tokio::spawn(future);

    Ok(())
}
struct Oneshot {
    tx: mpsc::Sender<Message>,
}

impl Oneshot {
    fn new(name: String, tx: mpsc::Sender<Message>) -> impl Future<Item = (), Error = ()> {
        future::ok(Message::State(State::Success))
            .and_then(move |msg| tx.send(msg))
            .map(|_| ())
            .map_err(|_| ())
    }
}

// struct OneShotFuture {}

// impl Future for OneShotFuture {
//     type Item = Message;
//     type Error = ();

//     fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
//         Ok(Async::Ready(Message::Done))
//     }
// }

// /// Process defines the process type
// pub enum Process {
//     OneShot(mpsc::Sender<Message>, OneShotFuture),
// }

// impl Future for Process {
//     type Item = ();
//     type Error = ();

//     fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
//         match self {
//             Process::OneShot(tx, ref mut handle) => {
//                 let out = try_ready!(handle.poll());
//                 tx.send(out);

//                 Ok(Async::Ready(()))
//             }
//         }
//     }
// }
