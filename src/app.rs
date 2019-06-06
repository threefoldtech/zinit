use crate::api;
use crate::manager;
use crate::settings;

use failure::Error;
use future::lazy;
use tokio::prelude::*;

type Result<T> = std::result::Result<T, Error>;

/// init start init command, immediately monitor all services
/// that are defined under the config directory
pub fn init(config: &str) -> Result<()> {
    // load config
    let configs = settings::load_dir(config, |file, err| {
        println!(
            "encountered err {} while loading file {:?}. skipping!",
            err, file
        );
        settings::Walk::Continue
    })?;

    // start the tokio runtime, start the process manager
    // and monitor all configured services
    // TODO:
    // We need to start the unix socket server that will
    // receive and handle user management commands (start, stop, status, etc...)
    tokio::run(lazy(|| {
        // creating a new instance from the process manager
        let manager = manager::Manager::new();

        // running the manager returns a handle that we can
        // use to actually control the process manager
        // currently the handle only exposes one method
        // `monitor` which spawns a new task on the process
        // manager given the configuration
        let handle = manager.run();

        for (name, config) in configs.into_iter() {
            handle.monitor(name, config);
        }

        if let Err(e) = api::run(handle) {
            error!("failed to start ctrl api {}", e);
        }

        Ok(())
    }));

    Ok(())
}
