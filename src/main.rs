use future::lazy;
use tokio::prelude::*;

mod manager;

use std::process::Command;

use tokio::prelude::*;
use tokio_process::CommandExt;

#[macro_use]
extern crate futures;

fn main() {
    let mut child = Command::new("ls").arg("-l").arg(".").status_async();

    let future = child
        .expect("failed to spawn")
        .map(|status| println!("the command exist with {}", status))
        .map_err(|_| ()); //let child = child.arg("-l").arg("/");
                          // child.status_async();
    tokio::run(future);
}

fn main2() {
    tokio::run(lazy(|| {
        let mut manager = manager::Manager::new();
        manager.run()
    }));
}
