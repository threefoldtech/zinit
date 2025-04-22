pub mod api;
pub mod rpc;

use crate::zinit;
use anyhow::{Context, Result};
use api::ApiServer;
use reth_ipc::client::IpcClientBuilder;
use rpc::ZinitLoggingApiClient;
use rpc::ZinitServiceApiClient;
use rpc::ZinitSystemApiClient;
use std::net::ToSocketAddrs;
use serde_yaml as encoder;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::time;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;
use tokio::signal;

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
) -> Result<ApiServer> {
    //std::fs::create_dir_all(config)?;
    if let Err(err) = logger(if debug {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    }) {
        eprintln!("failed to setup logging: {}", err);
    }

    let config = absolute(Path::new(config)).context("failed to get config dire absolute path")?;
    let socket_path =
        absolute(Path::new(socket)).context("failed to get socket file absolute path")?;

    if let Some(dir) = socket_path.parent() {
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
    let a = api::Api::new(init);
    a.serve(socket.into()).await
}

pub async fn list(socket: &str) -> Result<()> {
    let client = IpcClientBuilder::default().build(socket.into()).await?;
    let results = client.list().await?;
    encoder::to_writer(std::io::stdout(), &results)?;
    Ok(())
}

pub async fn shutdown(socket: &str) -> Result<()> {
    let client = IpcClientBuilder::default().build(socket.into()).await?;
    client.shutdown().await?;
    Ok(())
}

pub async fn reboot(socket: &str) -> Result<()> {
    let client = IpcClientBuilder::default().build(socket.into()).await?;
    client.reboot().await?;
    Ok(())
}

pub async fn status(socket: &str, name: String) -> Result<()> {
    let client = IpcClientBuilder::default().build(socket.into()).await?;
    let results = client.status(name).await?;
    encoder::to_writer(std::io::stdout(), &results)?;
    Ok(())
}

pub async fn start(socket: &str, name: String) -> Result<()> {
    let client = IpcClientBuilder::default().build(socket.into()).await?;
    client.start(name).await?;
    Ok(())
}

pub async fn stop(socket: &str, name: String) -> Result<()> {
    let client = IpcClientBuilder::default().build(socket.into()).await?;
    client.stop(name).await?;
    Ok(())
}

pub async fn restart(socket: &str, name: String) -> Result<()> {
    let client = IpcClientBuilder::default().build(socket.into()).await?;
    client.stop(name.clone()).await?;
    //pull status
    for _ in 0..20 {
        let result = client.status(name.clone()).await?;
        if result.pid == 0 && result.target == "Down" {
            client.start(name.clone()).await?;
            return Ok(());
        }
        time::sleep(std::time::Duration::from_secs(1)).await;
    }
    // process not stopped try to kill it
    client.kill(name.clone(), "SIGKILL".into()).await?;
    client.start(name).await?;
    Ok(())
}

pub async fn forget(socket: &str, name: String) -> Result<()> {
    let client = IpcClientBuilder::default().build(socket.into()).await?;
    client.forget(name).await?;
    Ok(())
}

pub async fn monitor(socket: &str, name: String) -> Result<()> {
    let client = IpcClientBuilder::default().build(socket.into()).await?;
    client.monitor(name).await?;
    Ok(())
}

pub async fn kill(socket: &str, name: String, signal: String) -> Result<()> {
    let client = IpcClientBuilder::default().build(socket.into()).await?;
    client.kill(name, signal).await?;
    Ok(())
}

pub async fn logs(socket: &str, filter: Option<String>, follow: bool) -> Result<impl Stream<Item = String> + Unpin> {
    let client = IpcClientBuilder::default().build(socket.into()).await?;
    if let Some(ref filter) = filter {
        client.status(filter.clone()).await?;
    }
    let logs = client.logs(filter.clone()).await?;
    let (tx, rx) = tokio::sync::mpsc::channel(2000);

    let logs_sub = if follow {
        Some(client.log_subscribe(filter).await?)
    } else {
        None
    };
    tokio::task::spawn(async move {
        for log in logs {
            if tx.send(log).await.is_err() {
                if let Some(logs_sub) = logs_sub {
                    let _ = logs_sub.unsubscribe().await;
                }
                // error means receiver is dead, so just quit
                return;
            }
        }
        let Some(mut logs_sub) = logs_sub else { return };
        loop {
            match logs_sub.next().await {
                Some(Ok(log)) => {
                    if tx.send(log).await.is_err() {
                        let _ = logs_sub.unsubscribe().await;
                        return
                    }
                }
                Some(Err(e)) => {
                    log::error!("Failed to get new log from subscription: {e}");
                    return;
                }
                _ => return,
            }
        }
    });

    Ok(ReceiverStream::new(rx))
}

/// Start an HTTP/RPC proxy server for the Zinit API at the specified address
pub async fn proxy(sock: &str, address: String) -> Result<()> {
    // Parse the socket address
    let _socket_addr = address.to_socket_addrs()
        .context("Failed to parse socket address")?
        .next()
        .context("No valid socket address found")?;

    println!("Starting HTTP/RPC server on {}", address);
    println!("Connecting to Zinit daemon at {}", sock);

    // Connect to the existing Zinit daemon through the Unix socket
    let client = IpcClientBuilder::default().build(sock.into()).await?;
    
    // Issue an RPC call to start the HTTP server on the specified address
    let result = client.start_http_server(address.clone()).await?;
    
    println!("{}", result);
    println!("Press Ctrl+C to stop");
    
    // Wait for Ctrl+C to shutdown
    signal::ctrl_c().await?;
    
    // Shutdown the HTTP server
    client.stop_http_server().await?;
    
    println!("HTTP/RPC server stopped");
    
    Ok(())
}