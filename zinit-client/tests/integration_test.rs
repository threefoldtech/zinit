use std::env;
use zinit_client::{Client, ClientError};

#[tokio::test]
async fn test_connection_error() {
    // Try to connect to a non-existent socket
    let result = Client::unix_socket("/non/existent/socket").await;
    assert!(result.is_ok()); // Just creating the client succeeds

    // Trying to make a request should fail
    if let Ok(client) = result {
        let list_result = client.list().await;
        assert!(matches!(list_result, Err(ClientError::ConnectionError(_))));
    }
}

#[tokio::test]
async fn test_http_connection_error() {
    // Try to connect to a non-existent HTTP endpoint
    let result = Client::http("http://localhost:12345").await;
    // This should succeed as we're just creating the client, not making a request
    assert!(result.is_ok());

    // Try to make a request which should fail
    if let Ok(client) = result {
        let list_result = client.list().await;
        assert!(matches!(list_result, Err(ClientError::ConnectionError(_))));
    }
}

// This test only runs if ZINIT_SOCKET is set in the environment
// and points to a valid Zinit socket
#[tokio::test]
#[ignore]
async fn test_live_connection() {
    let socket_path = match env::var("ZINIT_SOCKET") {
        Ok(path) => path,
        Err(_) => {
            println!("ZINIT_SOCKET not set, skipping live test");
            return;
        }
    };

    let client = match Client::unix_socket(&socket_path).await {
        Ok(client) => client,
        Err(e) => {
            panic!(
                "Failed to connect to Zinit socket at {}: {}",
                socket_path, e
            );
        }
    };

    // Test listing services
    let services = client.list().await.expect("Failed to list services");
    println!("Found {} services", services.len());

    // If there are services, test getting status of the first one
    if let Some((service_name, _)) = services.iter().next() {
        let status = client
            .status(service_name)
            .await
            .expect("Failed to get service status");
        println!("Service {} has PID {}", service_name, status.pid);
    }
}
