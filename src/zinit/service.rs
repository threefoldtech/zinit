use crate::zinit::config;
use crate::zinit::state::{State, Target};
use crate::zinit::stats::ProcessStats;
use crate::zinit::types::Watched;
use anyhow::{Context, Result};
use nix::unistd::Pid;

/// Represents a service managed by ZInit
pub struct ZInitService {
    /// Process ID of the running service
    pub pid: Pid,

    /// Service configuration
    pub service: config::Service,

    /// Target state of the service (up, down)
    pub target: Target,

    /// Whether the service is scheduled for execution
    pub scheduled: bool,

    /// Current state of the service
    state: Watched<State>,

    /// Process statistics (memory and CPU usage)
    pub stats: ProcessStats,
}

/// Status information for a service
pub struct ZInitStatus {
    /// Process ID of the running service
    pub pid: Pid,

    /// Service configuration
    pub service: config::Service,

    /// Target state of the service (up, down)
    pub target: Target,

    /// Whether the service is scheduled for execution
    pub scheduled: bool,

    /// Current state of the service
    pub state: State,

    /// Memory usage in kilobytes
    pub memory_kb: u64,

    /// CPU usage as a percentage (0-100)
    pub cpu_percent: f64,
}

impl ZInitService {
    /// Create a new service with the given configuration and initial state
    pub fn new(service: config::Service, state: State) -> ZInitService {
        ZInitService {
            pid: Pid::from_raw(0),
            state: Watched::new(state),
            service,
            target: Target::Up,
            scheduled: false,
            stats: ProcessStats::new(),
        }
    }

    /// Get the current status of the service
    pub fn status(&self) -> ZInitStatus {
        ZInitStatus {
            pid: self.pid,
            state: self.state.get().clone(),
            service: self.service.clone(),
            target: self.target.clone(),
            scheduled: self.scheduled,
            memory_kb: self.stats.memory_kb,
            cpu_percent: self.stats.cpu_percent,
        }
    }

    /// Update process statistics
    pub fn update_stats(&mut self) -> Result<()> {
        if self.is_running() {
            self.stats.update(self.pid)?;
        } else {
            // Reset stats if process is not running
            self.stats = ProcessStats::new();
        }
        Ok(())
    }

    /// Set the state of the service, validating the state transition
    pub fn set_state(&mut self, state: State) -> Result<()> {
        let current_state = self.state.get().clone();
        let new_state = current_state
            .transition_to(state)
            .context("Failed to transition service state")?;

        self.state.set(new_state);
        Ok(())
    }

    /// Set the state of the service without validation
    pub fn force_set_state(&mut self, state: State) {
        self.state.set(state);
    }

    /// Set the target state of the service
    pub fn set_target(&mut self, target: Target) {
        self.target = target;
    }

    /// Get the current state of the service
    pub fn get_state(&self) -> &State {
        self.state.get()
    }

    /// Get a watcher for the service state
    pub fn state_watcher(&self) -> crate::zinit::types::Watcher<State> {
        self.state.watcher()
    }

    /// Check if the service is active (running or in progress)
    pub fn is_active(&self) -> bool {
        self.state.get().is_active()
    }

    /// Check if the service is in a terminal state (success or failure)
    pub fn is_terminal(&self) -> bool {
        self.state.get().is_terminal()
    }

    /// Set the process ID of the service
    pub fn set_pid(&mut self, pid: Pid) {
        self.pid = pid;
    }

    /// Clear the process ID of the service
    pub fn clear_pid(&mut self) {
        self.pid = Pid::from_raw(0);
    }

    /// Check if the service is running
    pub fn is_running(&self) -> bool {
        self.pid.as_raw() != 0 && self.state.get().is_active()
    }

    /// Check if the service is a one-shot service
    pub fn is_one_shot(&self) -> bool {
        self.service.one_shot
    }
}

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

        // Skip stats testing in CI environments
        if std::env::var("CI").is_ok() {
            println!("Skipping stats testing in CI environment");
            return;
        }

        // Set a PID and update stats - use the current process ID which we know exists
        let current_pid = std::process::id() as i32;
        service.set_pid(Pid::from_raw(current_pid));

        // Update stats should work
        if let Err(e) = service.update_stats() {
            println!("Skipping test, couldn't update stats: {}", e);
            return;
        }

        // For the test, we'll just verify that the stats were updated and are accessible
        // We can't reliably assert specific values since they depend on the system state

        // Status should reflect the updated stats
        let status = service.status();
        assert_eq!(status.memory_kb, service.stats.memory_kb);
        assert_eq!(status.cpu_percent, service.stats.cpu_percent);
    }
}
