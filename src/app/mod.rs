pub mod client;
pub mod server;
pub mod types;
pub mod api_trait;

use self::client::Client;
use self::server::Api;
use crate::zinit;
use anyhow::{Context, Result};
use serde_yaml as encoder;
use std::net::SocketAddr; // Import SocketAddr
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::time;
use log::{debug, error}; // Import log macros if not already present at top level

fn logger(level: log::LevelFilter) -> Result<()> {
    let logger = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "zinit: {} ({}) {}",
                record.level(),
                record.target(),
                message
            ))
        })
        .level(level)
        .chain(std::io::stdout());
    let logger = match std::fs::OpenOptions::new().write(true).open("/dev/kmsg") {
        Ok(file) => logger.chain(file),
        Err(_err) => logger,
    };
    logger.apply()?;

    Ok(())
}

fn absolute<P: AsRef<Path>>(p: P) -> Result<PathBuf> {
    let p = p.as_ref();
    let result = if p.is_absolute() {
        p.to_path_buf()
    } else {
        let mut current = std::env::current_dir()?;
        current.push(p);
        current
    };

    Ok(result)
}

pub async fn init(
    cap: usize,
    config: &str,
    socket: &str,
    container: bool,
    debug: bool,
) -> Result<()> {
    //std::fs::create_dir_all(config)?;
    if let Err(err) = logger(if debug {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    }) {
        eprintln!("failed to setup logging: {}", err);
    }

    let config = absolute(Path::new(config)).context("failed to get config dir absolute path")?;
    let socket = absolute(Path::new(socket)).context("failed to get socket file absolute path")?;

    if let Some(dir) = socket.parent() {
        fs::create_dir_all(dir)
            .await
            .with_context(|| format!("failed to create directory {:?}", dir))?;
    }

    let _ = fs::remove_file(&socket).await;

    debug!("switching to home dir: {}", config.display());
    std::env::set_current_dir(&config).with_context(|| {
        format!(
            "failed to switch working directory to '{}'",
            config.display()
        )
    })?;

    let init = zinit::ZInit::new(cap, container);

    init.serve();

    let services = zinit::config::load_dir(&config)?;
    for (k, v) in services {
        if let Err(err) = init.monitor(&k, v).await {
            error!("failed to monitor service {}: {}", k, err);
        };
    }
    // Create Api instance with ZInit and the config directory path
    let api = Api::new(init, config.clone()); // Pass config dir, not socket

    // Define server addresses (consider making these configurable later)
    let http_addr: SocketAddr = "127.0.0.1:9000".parse().expect("Failed to parse HTTP address");
    let ws_addr: SocketAddr = "127.0.0.1:9001".parse().expect("Failed to parse WS address");

    // Start the jsonrpsee server
    let (server_handle, _bound_http_addr, _bound_ws_addr) = api.serve(http_addr, ws_addr).await?;

    // Keep the server running by awaiting its handle
    // This will block indefinitely until the server is stopped (e.g., by shutdown signal)
    server_handle.stopped().await;

    Ok(()) // This might not be reached if server runs indefinitely, but keep for structure
}
// Define default URLs (consider making these configurable via args later)
const DEFAULT_HTTP_URL: &str = "http://127.0.0.1:9000";
const DEFAULT_WS_URL: &str = "ws://127.0.0.1:9001";

pub async fn list() -> Result<()> {
    let client = Client::new(DEFAULT_HTTP_URL.to_string(), DEFAULT_WS_URL.to_string());
    let results = client.list().await?;
    encoder::to_writer(std::io::stdout(), &results)?;
    Ok(())
}
pub async fn shutdown() -> Result<()> {
    let client = Client::new(DEFAULT_HTTP_URL.to_string(), DEFAULT_WS_URL.to_string());
    client.shutdown().await?;
    Ok(())
}
pub async fn reboot() -> Result<()> {
    let client = Client::new(DEFAULT_HTTP_URL.to_string(), DEFAULT_WS_URL.to_string());
    client.reboot().await?;
    Ok(())
}
pub async fn status(name: &str) -> Result<()> {
    let client = Client::new(DEFAULT_HTTP_URL.to_string(), DEFAULT_WS_URL.to_string());
    let results = client.status(name).await?;
    encoder::to_writer(std::io::stdout(), &results)?;
    Ok(())
}
pub async fn start(name: &str) -> Result<()> {
    let client = Client::new(DEFAULT_HTTP_URL.to_string(), DEFAULT_WS_URL.to_string());
    client.start(name).await?;
    Ok(())
}
pub async fn stop(name: &str) -> Result<()> {
    let client = Client::new(DEFAULT_HTTP_URL.to_string(), DEFAULT_WS_URL.to_string());
    client.stop(name).await?;
    Ok(())
}
pub async fn restart(name: &str) -> Result<()> {
    let client = Client::new(DEFAULT_HTTP_URL.to_string(), DEFAULT_WS_URL.to_string());
    client.stop(name).await?;
    //pull status
    for _ in 0..20 {
        let result = client.status(name).await?;
        if result.pid == 0 && result.target == "Down" {
            client.start(name).await?;
            return Ok(());
        }
        time::sleep(std::time::Duration::from_secs(1)).await;
    }
    // process not stopped try to kill it
    client.kill(name, "SIGKILL").await?;
    client.start(name).await?;
    Ok(())
}
pub async fn forget(name: &str) -> Result<()> {
    let client = Client::new(DEFAULT_HTTP_URL.to_string(), DEFAULT_WS_URL.to_string());
    client.forget(name).await?;
    Ok(())
}
pub async fn monitor(name: &str) -> Result<()> {
    let client = Client::new(DEFAULT_HTTP_URL.to_string(), DEFAULT_WS_URL.to_string());
    client.monitor(name).await?;
    Ok(())
}
pub async fn kill(name: &str, signal: &str) -> Result<()> {
    let client = Client::new(DEFAULT_HTTP_URL.to_string(), DEFAULT_WS_URL.to_string());
    client.kill(name, signal).await?;
    Ok(())
}
pub async fn logs(filter: Option<&str>, follow: bool) -> Result<()> {
    // Logs uses WebSocket, so we pass the WS URL
    let client = Client::new(DEFAULT_HTTP_URL.to_string(), DEFAULT_WS_URL.to_string());
    if let Some(filter) = filter {
        client.status(filter).await?;
    }
    client.logs(tokio::io::stdout(), filter, follow).await
}
