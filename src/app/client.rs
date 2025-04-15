use crate::app::api_trait::{
    ZinitRpcApiClient, ZinitServiceApiClient, ZinitSystemApiClient,
};
use crate::app::types::Status;
use anyhow::{Context, Result, bail};
use jsonrpsee::{
    core::client::{SubscriptionClientT, Subscription},
    http_client::{HttpClient, HttpClientBuilder},
    ws_client::{WsClient, WsClientBuilder},
};
use jsonrpsee::rpc_params;
use log::error; // Use log crate
use serde_json::{Value, Map};
use std::collections::HashMap;
use std::time::Duration; // For timeouts
use tokio::io::AsyncWriteExt; // For logs output

// Client struct now holds URLs
pub struct Client {
    http_url: String,
    ws_url: String,
    // We can store created clients if we want to reuse connections,
    // but creating them on demand is simpler for now.
}

impl Client {
    // Constructor now takes HTTP and WS URLs
    pub fn new(http_url: String, ws_url: String) -> Self {
        // Basic validation or parsing could be added here
        Client { http_url, ws_url }
    }

    // Helper to create an HTTP client
    async fn http_client(&self) -> Result<HttpClient> {
        HttpClientBuilder::default()
            .request_timeout(Duration::from_secs(30)) // Example timeout
            .build(&self.http_url)
            .context("Failed to build HTTP client")
    }

    // Helper to create a WS client
    async fn ws_client(&self) -> Result<WsClient> {
        // Use WsTransportClientBuilder for custom headers if needed later
        WsClientBuilder::default()
            .request_timeout(Duration::from_secs(30)) // Example timeout
            .build(&self.ws_url)
            .await // build() is async for WsClient
            .context("Failed to build WebSocket client")
    }

    // --- Service Methods ---

    pub async fn list(&self) -> Result<HashMap<String, String>> {
        let client = self.http_client().await?;
        client
            .list() // Call the trait method
            .await
            .context("RPC call 'service.list' failed")
    }

    pub async fn status<S: AsRef<str>>(&self, name: S) -> Result<Status> {
        let client = self.http_client().await?;
        client
            .status(name.as_ref().to_string()) // Pass name as String
            .await
            .context("RPC call 'service.status' failed")
    }

    pub async fn start<S: AsRef<str>>(&self, name: S) -> Result<()> {
        let client = self.http_client().await?;
        client
            .start(name.as_ref().to_string())
            .await
            .context("RPC call 'service.start' failed")
    }

    pub async fn stop<S: AsRef<str>>(&self, name: S) -> Result<()> {
        let client = self.http_client().await?;
        client
            .stop(name.as_ref().to_string())
            .await
            .context("RPC call 'service.stop' failed")
    }

    pub async fn monitor<S: AsRef<str>>(&self, name: S) -> Result<()> {
        let client = self.http_client().await?;
        client
            .monitor(name.as_ref().to_string())
            .await
            .context("RPC call 'service.monitor' failed")
    }

    pub async fn forget<S: AsRef<str>>(&self, name: S) -> Result<()> {
        let client = self.http_client().await?;
        client
            .forget(name.as_ref().to_string())
            .await
            .context("RPC call 'service.forget' failed")
    }

    pub async fn kill<S: AsRef<str>>(&self, name: S, sig: S) -> Result<()> {
        let client = self.http_client().await?;
        client
            .kill(name.as_ref().to_string(), sig.as_ref().to_string())
            .await
            .context("RPC call 'service.kill' failed")
    }

    // --- Service File Methods ---

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

    pub async fn delete_service<S: AsRef<str>>(&self, name: S) -> Result<String> {
        let client = self.http_client().await?;
        client
            .delete(name.as_ref().to_string())
            .await
            .context("RPC call 'service.delete' failed")
    }

    pub async fn get_service<S: AsRef<str>>(&self, name: S) -> Result<Value> {
        let client = self.http_client().await?;
        client
            .get(name.as_ref().to_string())
            .await
            .context("RPC call 'service.get' failed")
    }

    // --- System Methods ---

    pub async fn shutdown(&self) -> Result<()> {
        let client = self.http_client().await?;
        client
            .shutdown()
            .await
            .context("RPC call 'system.shutdown' failed")
    }

    pub async fn reboot(&self) -> Result<()> {
        let client = self.http_client().await?;
        client
            .reboot()
            .await
            .context("RPC call 'system.reboot' failed")
    }

    // --- Logging Subscription ---

    pub async fn logs<O: AsyncWriteExt + Unpin, S: AsRef<str>>(
        &self,
        mut out: O,
        filter: Option<S>,
        follow: bool, // 'follow' now determines if we subscribe or just fetch once (if needed)
    ) -> Result<()> {
        if !follow {
            // If not following, we might need a way to fetch a snapshot.
            // The current API trait only defines a subscription.
            // Option 1: Add a separate `logs_snapshot` method to the RPC API.
            // Option 2: Subscribe, get the initial burst, then unsubscribe (complex).
            // Option 3: For now, just error if not following, as we only have subscribe.
            bail!("Non-following log retrieval is not currently supported via WebSocket subscription. Use follow=true.");
            // OR implement snapshot logic if added to API trait later.
        }

        let client = self.ws_client().await?;

        // Pass filter as parameter during subscription
        let filter_param = filter.map(|s| s.as_ref().to_string());
        // Parameters are not needed for this subscription method

        // Subscribe using the core subscribe method to pass parameters
        let mut subscription: Subscription<String> = client
            .subscribe("stream_log", rpc_params![filter_param], "stream_log_unsubscribe") // Pass filter_param as the parameter
            .await
            .context("Failed to subscribe to logs")?;

        // println!("Subscribed to logs... Filter: {:?}", filter_param); // User feedback

        // Process messages from the subscription
        loop {
            tokio::select! {
                // Wait for ctrl-c signal to gracefully exit (optional but good practice)
                _ = tokio::signal::ctrl_c() => {
                    println!("Ctrl-C received, stopping log stream.");
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
                            println!("Log stream ended.");
                            break;
                        }
                    }
                }
            }
        }

        // Unsubscribe is handled automatically when the Subscription object is dropped.
        Ok(())
    }

     // --- RPC Discover Method ---
     pub async fn discover(&self) -> Result<Value> {
        let client = self.http_client().await?;
        client
            .discover()
            .await
            .context("RPC call 'rpc.discover' failed")
    }
}
