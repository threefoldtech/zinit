// Note: These tests require a running Zinit instance to work properly
// They are disabled by default to avoid failing in CI environments

#[cfg(test)]
mod tests {
    use zinit_client::Client;
    use std::env;

    // This test is ignored by default since it requires a running Zinit instance
    #[tokio::test]
    #[ignore]
    async fn test_unix_socket_list() {
        // Get the socket path from environment or use default
        let socket_path = env::var("ZINIT_SOCKET").unwrap_or_else(|_| "/var/run/zinit.sock".to_string());
        
        let client = Client::unix_socket(socket_path);
        let result = client.list().await;
        
        assert!(result.is_ok(), "Failed to list services: {:?}", result.err());
        let services = result.unwrap();
        println!("Found {} services", services.len());
        
        // Print the services
        for (name, state) in services {
            println!("- {}: {}", name, state);
        }
    }

    // This test is ignored by default since it requires a running Zinit HTTP proxy
    #[tokio::test]
    #[ignore]
    async fn test_http_list() {
        // Get the HTTP URL from environment or use default
        let http_url = env::var("ZINIT_HTTP").unwrap_or_else(|_| "http://localhost:8080".to_string());
        
        let client = Client::http(http_url);
        let result = client.list().await;
        
        assert!(result.is_ok(), "Failed to list services: {:?}", result.err());
        let services = result.unwrap();
        println!("Found {} services", services.len());
        
        // Print the services
        for (name, state) in services {
            println!("- {}: {}", name, state);
        }
    }

    // This test is ignored by default since it requires a running Zinit instance
    // and will actually start/stop a service
    #[tokio::test]
    #[ignore]
    async fn test_service_lifecycle() {
        // Get the socket path from environment or use default
        let socket_path = env::var("ZINIT_SOCKET").unwrap_or_else(|_| "/var/run/zinit.sock".to_string());
        // Get a test service name from environment
        let service_name = env::var("TEST_SERVICE").expect("TEST_SERVICE environment variable must be set");
        
        let client = Client::unix_socket(socket_path);
        
        // Test service status
        let status_result = client.status(&service_name).await;
        assert!(status_result.is_ok(), "Failed to get service status: {:?}", status_result.err());
        
        // Test service stop
        let stop_result = client.stop(&service_name).await;
        assert!(stop_result.is_ok(), "Failed to stop service: {:?}", stop_result.err());
        
        // Wait a bit for the service to stop
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        
        // Test service start
        let start_result = client.start(&service_name).await;
        assert!(start_result.is_ok(), "Failed to start service: {:?}", start_result.err());
        
        // Wait a bit for the service to start
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        
        // Check service status again
        let status_result = client.status(&service_name).await;
        assert!(status_result.is_ok(), "Failed to get service status: {:?}", status_result.err());
        let status = status_result.unwrap();
        
        // Service should be running
        assert!(status.pid > 0, "Service is not running");
    }
}