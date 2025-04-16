use anyhow::{Context, Result};
use std::path::Path;
use tokio::time::{sleep, Duration};
use std::env;
use tokio::process::Command;
use tokio::fs;
use std::process::Stdio;
use serde::{Deserialize, Serialize};
use serde_json;
use tokio::net::UnixStream;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufStream};
use std::collections::HashMap;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
struct Response {
    pub state: State,
    pub body: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum State {
    Ok,
    Error,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
struct Status {
    pub name: String,
    pub pid: u32,
    pub state: String,
    pub target: String,
    pub after: HashMap<String, String>,
}

struct Client {
    socket: String,
}

impl Client {
    pub fn new(socket: &str) -> Client {
        Client {
            socket: socket.to_string(),
        }
    }

    async fn connect(&self) -> Result<UnixStream> {
        UnixStream::connect(&self.socket).await.with_context(|| {
            format!(
                "failed to connect to '{}'. is zinit listening on that socket?",
                self.socket
            )
        })
    }

    async fn command(&self, c: &str) -> Result<serde_json::Value> {
        let mut con = BufStream::new(self.connect().await?);

        let _ = con.write(c.as_bytes()).await?;
        let _ = con.write(b"\n").await?;
        con.flush().await?;

        let mut data = String::new();
        con.read_to_string(&mut data).await?;

        let response: Response = serde_json::from_str(&data)?;

        match response.state {
            State::Ok => Ok(response.body),
            State::Error => {
                let err: String = serde_json::from_value(response.body)?;
                anyhow::bail!(err)
            }
        }
    }

    pub async fn list(&self) -> Result<HashMap<String, String>> {
        let response = self.command("list").await?;
        Ok(serde_json::from_value(response)?)
    }

    pub async fn status<S: AsRef<str>>(&self, name: S) -> Result<Status> {
        let response = self.command(&format!("status {}", name.as_ref())).await?;
        Ok(serde_json::from_value(response)?)
    }

    pub async fn start<S: AsRef<str>>(&self, name: S) -> Result<()> {
        self.command(&format!("start {}", name.as_ref())).await?;
        Ok(())
    }

    pub async fn stop<S: AsRef<str>>(&self, name: S) -> Result<()> {
        self.command(&format!("stop {}", name.as_ref())).await?;
        Ok(())
    }

    pub async fn forget<S: AsRef<str>>(&self, name: S) -> Result<()> {
        self.command(&format!("forget {}", name.as_ref())).await?;
        Ok(())
    }

    pub async fn monitor<S: AsRef<str>>(&self, name: S) -> Result<()> {
        self.command(&format!("monitor {}", name.as_ref())).await?;
        Ok(())
    }

    pub async fn kill<S: AsRef<str>>(&self, name: S, sig: S) -> Result<()> {
        self.command(&format!("kill {} {}", name.as_ref(), sig.as_ref()))
            .await?;
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.command("shutdown").await?;
        Ok(())
    }
}

async fn start_zinit(socket_path: &str, config_dir: &str) -> Result<()> {
    // Create a temporary config directory if it doesn't exist
    let config_path = Path::new(config_dir);
    if !config_path.exists() {
        fs::create_dir_all(config_path).await?;
    }

    // Start zinit in the background
    let mut cmd = Command::new("zinit");
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

async fn create_service_config(config_dir: &str, name: &str, command: &str) -> Result<()> {
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

    fs::write(config_path, config_content).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Define paths for socket and config
    let temp_dir = env::temp_dir();
    let socket_path = temp_dir.join("zinit-test.sock").to_str().unwrap().to_string();
    let config_dir = temp_dir.join("zinit-test-config").to_str().unwrap().to_string();

    println!("Starting zinit with socket at: {}", socket_path);
    println!("Using config directory: {}", config_dir);

    // Start zinit in the background
    start_zinit(&socket_path, &config_dir).await?;

    // Wait for zinit to initialize
    sleep(Duration::from_secs(2)).await;

    // Create a client to communicate with zinit
    let client = Client::new(&socket_path);

    // Create service configurations
    println!("Creating service configurations...");
    
    // Create a find service
    create_service_config(&config_dir, "find-service", "find / -name \"*.txt\" -type f").await?;
    
    // Create a sleep service with echo
    create_service_config(
        &config_dir, 
        "sleep-service", 
        "sh -c 'echo Starting sleep; sleep 30; echo Finished sleep'"
    ).await?;

    // Wait for zinit to load the configurations
    sleep(Duration::from_secs(1)).await;

    // Tell zinit to monitor our services
    println!("Monitoring services...");
    client.monitor("find-service").await?;
    client.monitor("sleep-service").await?;

    // List all services
    println!("\nListing all services:");
    let services = client.list().await?;
    for (name, status) in services {
        println!("Service: {} - Status: {}", name, status);
    }

    // Start the find service
    println!("\nStarting find-service...");
    client.start("find-service").await?;
    
    // Wait a bit and check status
    sleep(Duration::from_secs(2)).await;
    let status = client.status("find-service").await?;
    println!("find-service status: {:?}", status);

    // Start the sleep service
    println!("\nStarting sleep-service...");
    client.start("sleep-service").await?;
    
    // Wait a bit and check status
    sleep(Duration::from_secs(2)).await;
    let status = client.status("sleep-service").await?;
    println!("sleep-service status: {:?}", status);

    // Stop the find service
    println!("\nStopping find-service...");
    client.stop("find-service").await?;
    
    // Wait a bit and check status
    sleep(Duration::from_secs(2)).await;
    let status = client.status("find-service").await?;
    println!("find-service status after stopping: {:?}", status);

    // Kill the sleep service with SIGTERM
    println!("\nKilling sleep-service with SIGTERM...");
    client.kill("sleep-service", "SIGTERM").await?;
    
    // Wait a bit and check status
    sleep(Duration::from_secs(2)).await;
    let status = client.status("sleep-service").await?;
    println!("sleep-service status after killing: {:?}", status);

    // Cleanup - forget services
    println!("\nForgetting services...");
    if status.pid == 0 {  // Only forget if it's not running
        client.forget("sleep-service").await?;
    }
    client.forget("find-service").await?;

    // Shutdown zinit
    println!("\nShutting down zinit...");
    client.shutdown().await?;

    println!("\nTest completed successfully!");
    Ok(())
}