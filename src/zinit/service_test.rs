#[cfg(test)]
mod tests {
    use super::*;
    use crate::zinit::config;
    use crate::zinit::state::State;
    use nix::unistd::Pid;

    #[test]
    fn test_service_with_stats() {
        // Create a basic service configuration
        let service_config = config::Service {
            exec: "test".to_string(),
            dir: "".to_string(),
            env: std::collections::HashMap::new(),
            after: vec![],
            one_shot: false,
            test: "".to_string(),
            log: config::Log::None,
            signal: config::Signal {
                stop: "SIGTERM".to_string(),
            },
            shutdown_timeout: 10,
        };

        // Create a new service
        let mut service = ZInitService::new(service_config, State::Unknown);

        // Initially stats should be zero
        assert_eq!(service.stats.memory_kb, 0);
        assert_eq!(service.stats.cpu_percent, 0.0);

        // Status should include the stats
        let status = service.status();
        assert_eq!(status.memory_kb, 0);
        assert_eq!(status.cpu_percent, 0.0);

        // Set a PID and update stats
        service.set_pid(Pid::from_raw(std::process::id() as i32));

        // Update stats should work
        let result = service.update_stats();
        assert!(result.is_ok());

        // After updating, memory should be greater than 0
        assert!(service.stats.memory_kb > 0);

        // Status should reflect the updated stats
        let status = service.status();
        assert!(status.memory_kb > 0);
    }
}
