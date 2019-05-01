use future::lazy;
use tokio::prelude::*;

mod manager;
mod settings;

use std::process::Command;

use tokio::prelude::*;
use tokio_process::CommandExt;

#[macro_use]
extern crate futures;

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
                exec: "date".to_string(),
                one_shot: false,
                after: vec![],
            },
        );

        Ok(())
    }));
}
