use anyhow::Result;
use std::path::Path;
use tokio::process::Command;
use tokio::time::{sleep, Duration};
use std::process::Stdio;
use std::env;

pub async fn start_zinit(socket_path: &str, config_dir: &str) -> Result<()> {
    // Create a temporary config directory if it doesn't exist
    let config_path = Path::new(config_dir);
    if !config_path.exists() {
        tokio::fs::create_dir_all(config_path).await?;
    }

    // Get the path to the zinit binary (use the one we just built)
    let zinit_path = env::current_dir()?.join("target/debug/zinit");
    println!("Using zinit binary at: {}", zinit_path.display());

    // Start zinit in the background
    let mut cmd = Command::new(zinit_path);
    cmd.arg("--socket")
        .arg(socket_path)
        .arg("init")
        .arg("--config")
        .arg(config_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = cmd.spawn()?;
    
    // Give zinit some time to start up
    sleep(Duration::from_secs(1)).await;
    
    println!("Zinit started with PID: {:?}", child.id());
    
    Ok(())
}

pub async fn create_service_config(config_dir: &str, name: &str, command: &str) -> Result<()> {
    let config_path = format!("{}/{}.yaml", config_dir, name);
    let config_content = format!(
        r#"exec: {}
oneshot: false
shutdown_timeout: 10
after: []
signal:
  stop: sigterm
log: ring
env: {{}}
dir: /
"#,
        command
    );

    tokio::fs::write(config_path, config_content).await?;
    Ok(())
}