use crate::zinit::config;
use crate::zinit::state::{State, Target};
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
        }
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
