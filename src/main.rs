use future::lazy;
use tokio::prelude::*;

#[allow(dead_code)]
mod manager;
#[allow(dead_code)]
mod settings;

extern crate futures;
#[macro_use]
extern crate failure;

fn main() {
    tokio::run(lazy(|| {
        // creating a new instance from the process manager
        let manager = manager::Manager::new();

        // running the manager returns a handle that we can
        // use to actually control the process manager
        // currently the handle only exposes one method
        // `monitor` which spawns a new task on the process
        // manager given the configuration
        let handle = manager.run();

        // example long running service with test
        // handle.monitor(
        //     "test".to_string(),
        //     settings::Service {
        //         exec: String::from("bash -c 'sleep 0.5s; redis-server'"),
        //         test: String::from("redis-cli ping"),
        //         one_shot: false,
        //         after: vec![],
        //     },
        // );

        // example service with dependencies
        // the 3 together will print `hello world\n`

        handle.monitor(
            "third".to_string(),
            settings::Service {
                exec: String::from("echo"),
                test: String::from(""),
                one_shot: true,
                after: vec![String::from("first"), String::from("second")],
            },
        );

        handle.monitor(
            "second".to_string(),
            settings::Service {
                exec: String::from("echo -n world"),
                test: String::from(""),
                one_shot: true,
                after: vec![String::from("first")],
            },
        );

        handle.monitor(
            "first".to_string(),
            settings::Service {
                exec: String::from("echo -n 'hello '"),
                test: String::from(""),
                one_shot: true,
                after: vec![],
            },
        );

        Ok(())
    }));
}
