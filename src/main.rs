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
        handle.monitor(
            "test".to_string(),
            settings::Service {
                exec: String::from("bash -c 'sleep 0.5s; redis-server'"),
                test: String::from("redis-cli ping"),
                one_shot: false,
                after: vec![],
            },
        );

        Ok(())
    }));
}
