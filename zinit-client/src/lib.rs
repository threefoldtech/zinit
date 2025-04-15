//! Zinit Client Library
//!
//! A client library for interacting with Zinit process manager using JSON-RPC over HTTP and WebSockets.

use anyhow::{Context, Result, bail};
use jsonrpsee::{
    core::client::{ClientT, SubscriptionClientT, Subscription}, // Core client traits
    http_client::{HttpClient, HttpClientBuilder},
    ws_client::{WsClient, WsClientBuilder},
};
use log::{error, info, warn}; // Use log crate
use serde_json::{Value, Map};
use std::collections::HashMap;
use std::time::Duration; // For timeouts
use tokio::io::AsyncWriteExt; // For logs output
use url::Url; // For parsing URLs
use jsonrpsee::rpc_params;

// Re-export types and traits from the main zinit crate
// This assumes the `pub use` statements were added to zinit/src/lib.rs
pub use zinit::{
    Status, // Re-export Status type
    ZinitRpcApiClient, ZinitServiceApiClient, ZinitSystemApiClient, ZinitLoggingApiClient,
};

/// Zinit client for interacting with the Zinit process manager
#[derive(Debug, Clone)] // Make client cloneable if needed
pub struct Client {
    http_url: String,
    ws_url: String,
    // Optional: Add timeout configurations here
}

impl Client {
    /// Create a new client with specified HTTP and WebSocket URLs.
    ///
    /// # Arguments
    ///
    /// * `http_url` - The URL for the Zinit HTTP JSON-RPC endpoint (e.g., "http://127.0.0.1:9000").
    /// * `ws_url` - The URL for the Zinit WebSocket JSON-RPC endpoint (e.g., "ws://127.0.0.1:9001").
    pub fn new(http_url: String, ws_url: String) -> Result<Self> {
        // Validate URLs
        Url::parse(&http_url).context("Invalid HTTP URL")?;
        Url::parse(&ws_url).context("Invalid WebSocket URL")?;
        Ok(Client { http_url, ws_url })
    }

    /// Create a new client using default local URLs:
    /// HTTP: "http://127.0.0.1:9000"
    /// WS:   "ws://127.0.0.1:9001"
    pub fn default_local() -> Result<Self> {
        Self::new(
            "http://127.0.0.1:9000".to_string(),
            "ws://127.0.0.1:9001".to_string(),
        )
    }

    // Helper to create an HTTP client (consider caching later if performance is critical)
    async fn http_client(&self) -> Result<HttpClient> {
        HttpClientBuilder::default()
            .request_timeout(Duration::from_secs(30)) // Example timeout
            .build(&self.http_url)
            .context("Failed to build HTTP client")
    }

    // Helper to create a WS client (consider caching later)
    async fn ws_client(&self) -> Result<WsClient> {
        WsClientBuilder::default()
            .request_timeout(Duration::from_secs(30)) // Example timeout
            .connection_timeout(Duration::from_secs(10)) // Connection specific timeout
            .build(&self.ws_url)
            .await // build() is async for WsClient
            .context("Failed to build WebSocket client")
    }

    // --- Service Methods (using ZinitServiceApiClient) ---

    /// List all monitored services and their current state.
    /// Returns a map where keys are service names and values are state strings.
    pub async fn list(&self) -> Result<HashMap<String, String>> {
        let client = self.http_client().await?;
        client
            .list() // Call the trait method
            .await
            .context("RPC call 'service.list' failed")
    }

    /// Get the detailed status of a specific service.
    pub async fn status<S: AsRef<str>>(&self, name: S) -> Result<Status> {
        let client = self.http_client().await?;
        client
            .status(name.as_ref().to_string()) // Pass name as String
            .await
            .context("RPC call 'service.status' failed")
    }

    /// Start a specific service.
    pub async fn start<S: AsRef<str>>(&self, name: S) -> Result<()> {
        let client = self.http_client().await?;
        client
            .start(name.as_ref().to_string())
            .await
            .context("RPC call 'service.start' failed")
    }

    /// Stop a specific service.
    pub async fn stop<S: AsRef<str>>(&self, name: S) -> Result<()> {
        let client = self.http_client().await?;
        client
            .stop(name.as_ref().to_string())
            .await
            .context("RPC call 'service.stop' failed")
    }

    /// Load and monitor a new service from its configuration file on the server.
    pub async fn monitor<S: AsRef<str>>(&self, name: S) -> Result<()> {
        let client = self.http_client().await?;
        client
            .monitor(name.as_ref().to_string())
            .await
            .context("RPC call 'service.monitor' failed")
    }

    /// Stop monitoring a service and remove it from management.
    pub async fn forget<S: AsRef<str>>(&self, name: S) -> Result<()> {
        let client = self.http_client().await?;
        client
            .forget(name.as_ref().to_string())
            .await
            .context("RPC call 'service.forget' failed")
    }

    /// Send a signal (e.g., "SIGTERM", "SIGKILL") to a specific service process.
    pub async fn kill<S: AsRef<str>>(&self, name: S, sig: S) -> Result<()> {
        let client = self.http_client().await?;
        client
            .kill(name.as_ref().to_string(), sig.as_ref().to_string())
            .await
            .context("RPC call 'service.kill' failed")
    }

    /// Restart a service (stops, waits for down, starts).
    pub async fn restart<S: AsRef<str>>(&self, name: S) -> Result<()> {
        let name_ref = name.as_ref();
        info!("Attempting to restart service '{}'", name_ref);

        // 1. Stop the service
        self.stop(name_ref).await.context("Failed to stop service during restart")?;
        info!("Stop command sent for service '{}'", name_ref);

        // 2. Wait for the service to be down
        let mut attempts = 0;
        let max_attempts = 20; // ~20 seconds timeout
        loop {
            if attempts >= max_attempts {
                warn!("Service '{}' did not stop within timeout. Attempting kill.", name_ref);
                self.kill(name_ref, "SIGKILL").await.context("Failed to send SIGKILL during restart")?;
                // Add a small delay after kill before starting
                tokio::time::sleep(Duration::from_secs(1)).await;
                break; // Proceed to start after kill attempt
            }
            match self.status(name_ref).await {
                Ok(status) => {
                    info!("Current status for '{}': pid={}, state={}", name_ref, status.pid, status.state);
                    // Check state string representation (adjust if Status.state format changes)
                    if status.pid == 0 && status.state.eq_ignore_ascii_case("down") {
                        info!("Service '{}' confirmed down.", name_ref);
                        break; // Service is stopped
                    }
                }
                Err(e) => {
                    // Handle case where status fails (e.g., service forgotten during restart)
                    warn!("Failed to get status for '{}' during restart check: {}. Assuming stopped.", name_ref, e);
                    break;
                }
            }
            attempts += 1;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        // 3. Start the service
        info!("Attempting to start service '{}'", name_ref);
        self.start(name_ref).await.context("Failed to start service during restart")?;
        info!("Start command sent for service '{}'", name_ref);

        Ok(())
    }


    // --- Service File Methods (using ZinitServiceApiClient) ---

    /// Create a new service configuration file on the server.
    /// Returns a success message string.
    pub async fn create_service<S: AsRef<str>>(
        &self,
        name: S,
        content: Map<String, Value>,
    ) -> Result<String> {
        let client = self.http_client().await?;
        client
            .create(name.as_ref().to_string(), content)
            .await
            .context("RPC call 'service.create' failed")
    }

    /// Delete a service configuration file on the server.
    /// Returns a success message string.
    pub async fn delete_service<S: AsRef<str>>(&self, name: S) -> Result<String> {
        let client = self.http_client().await?;
        client
            .delete(name.as_ref().to_string())
            .await
            .context("RPC call 'service.delete' failed")
    }

    /// Get the content of a service configuration file from the server as a JSON Value.
    pub async fn get_service<S: AsRef<str>>(&self, name: S) -> Result<Value> {
        let client = self.http_client().await?;
        client
            .get(name.as_ref().to_string())
            .await
            .context("RPC call 'service.get' failed")
    }

    // --- System Methods (using ZinitSystemApiClient) ---

    /// Initiate system shutdown process on the server.
    pub async fn shutdown(&self) -> Result<()> {
        let client = self.http_client().await?;
        client
            .shutdown()
            .await
            .context("RPC call 'system.shutdown' failed")
    }

    /// Initiate system reboot process on the server.
    pub async fn reboot(&self) -> Result<()> {
        let client = self.http_client().await?;
        client
            .reboot()
            .await
            .context("RPC call 'system.reboot' failed")
    }

    // --- Logging Subscription (using ZinitLoggingApiClient) ---

    /// Subscribe to log messages from the Zinit server.
    ///
    /// # Arguments
    ///
    /// * `out` - An asynchronous writer where log lines will be written.
    /// * `filter` - An optional string. If provided, only logs containing this string will be forwarded.
    ///
    /// This function will run indefinitely, streaming logs until the connection is closed
    /// or an error occurs. Consider running it in a separate task.
    pub async fn logs<O: AsyncWriteExt + Unpin, S: AsRef<str>>(
        &self,
        mut out: O,
        filter: Option<S>,
    ) -> Result<()> {
        let client = self.ws_client().await?;
        info!("Attempting to subscribe to logs via WebSocket: {}", self.ws_url);

        // Pass filter as parameter during subscription
        let filter_param = filter.map(|s| s.as_ref().to_string());
        // Parameters must be JSON array or object, depending on server expectation.
        // Our trait expects an optional string, let's wrap it in a JSON array.
        let params = vec![serde_json::to_value(filter_param.clone())?];

        // Subscribe using the core subscribe method to pass parameters
        let mut subscription: Subscription<String> = client
            .subscribe("stream_log", rpc_params![filter_param], "stream_log_unsubscribe") // Use method name, params, unsubscribe name
            .await
            .context("Failed to subscribe to logs")?;

        // info!("Successfully subscribed to logs. Filter: {:?}", filter_param);

        // Process messages from the subscription
        loop {
            tokio::select! {
                // Allow breaking loop externally if needed (e.g., ctrl-c in an example)
                _ = tokio::signal::ctrl_c() => {
                    info!("Ctrl-C received, stopping log stream.");
                    break;
                }
                // Get next item from subscription stream
                item = subscription.next() => {
                    match item {
                        Some(Ok(log_line)) => {
                            // Write log line to output
                            if let Err(e) = out.write_all(log_line.as_bytes()).await {
                                error!("Failed to write log line to output: {}", e);
                                break; // Stop if output fails
                            }
                            if let Err(e) = out.write_all(b"\n").await {
                                error!("Failed to write newline to output: {}", e);
                                break;
                            }
                            if let Err(e) = out.flush().await {
                                error!("Failed to flush output: {}", e);
                                break;
                            }
                        }
                        Some(Err(e)) => {
                            // Handle errors from the subscription stream itself
                            error!("Error receiving log line: {}", e);
                            // Depending on the error, might want to break or retry
                            break;
                        }
                        None => {
                            // Subscription stream closed by the server
                            info!("Log stream ended (connection closed by server).");
                            break;
                        }
                    }
                }
            }
        }

        // Unsubscribe is handled automatically when the Subscription object is dropped.
        info!("Log subscription finished.");
        Ok(())
    }

     // --- RPC Discover Method (using ZinitRpcApiClient) ---

     /// Returns the OpenRPC specification from the server as a JSON Value.
     pub async fn discover(&self) -> Result<Value> {
        let client = self.http_client().await?;
        client
            .discover()
            .await
            .context("RPC call 'rpc.discover' failed")
    }
}
