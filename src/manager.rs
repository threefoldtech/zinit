use futures::{future, lazy};
use std::collections::HashMap;
use std::time::Duration;
use tokio::prelude::*;
use tokio::sync::mpsc;
use tokio::timer;

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
}

/// Manager is the main entry point, it keeps track of the
/// processes state, and spawn them based on the dependencies.
pub struct Manager {
    states: HashMap<String, State>,
    tx: mpsc::Sender<Message>,
    rx: Option<mpsc::Receiver<Message>>,
}

impl Manager {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(10);

        Manager {
            states: HashMap::new(),
            tx: tx,
            rx: Some(rx),
        }
    }

    fn keep_alive(&self) {
        // this ticker is just to make sure the tokio runtime will never shutdown
        // even if no more services are being monitored.
        // By having a long living future that will never exit
        tokio::spawn(lazy(move || {
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

    pub fn monitor(&mut self, name: String, cmd: String) {
        self.states.insert(name.clone(), State::Scheduled);
        let tx = self.tx.clone();

        tokio::spawn(Oneshot::new(name, tx));
    }

    pub fn run(&mut self) -> impl Future<Item = (), Error = ()> {
        println!("running manager");

        let rx = match self.rx.take() {
            Some(rx) => rx,
            None => panic!("manager is already running"),
        };

        self.keep_alive();

        rx.for_each(|msg| {
            println!("Message: {:?}", msg);
            Ok(())
        })
        .map_err(|e| {
            println!("error: {}", e);
            ()
        })
    }
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
