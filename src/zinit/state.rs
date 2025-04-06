use crate::zinit::errors::ZInitError;
use anyhow::Result;
use nix::sys::wait::WaitStatus;

/// Target state for a service
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// Service should be running
    Up,
    /// Service should be stopped
    Down,
}

/// Service state
#[derive(Debug, PartialEq, Clone)]
pub enum State {
    /// Service is in an unknown state
    Unknown,

    /// Blocked means one or more dependencies hasn't been met yet. Service can stay in
    /// this state as long as at least one dependency is not in either Running, or Success
    Blocked,

    /// Service has been started, but it didn't exit yet, or we didn't run the test command.
    Spawned,

    /// Service has been started, and test command passed.
    Running,

    /// Service has exited with success state, only one-shot can stay in this state
    Success,

    /// Service exited with this error, only one-shot can stay in this state
    Error(WaitStatus),

    /// The service test command failed, this might (or might not) be replaced
    /// with an Error state later on once the service process itself exits
    TestFailure,

    /// Failure means the service has failed to spawn in a way that retrying
    /// won't help, like command line parsing error or failed to fork
    Failure,
}

impl State {
    /// Validate if a transition from the current state to the new state is valid
    pub fn can_transition_to(&self, new_state: &State) -> bool {
        match (self, new_state) {
            // From Unknown state, any transition is valid
            (State::Unknown, _) => true,

            // From Blocked state
            (State::Blocked, State::Spawned) => true,
            (State::Blocked, State::Failure) => true,

            // From Spawned state
            (State::Spawned, State::Running) => true,
            (State::Spawned, State::TestFailure) => true,
            (State::Spawned, State::Error(_)) => true,
            (State::Spawned, State::Success) => true,

            // From Running state
            (State::Running, State::Success) => true,
            (State::Running, State::Error(_)) => true,

            // To Unknown or Blocked state is always valid
            (_, State::Unknown) => true,
            (_, State::Blocked) => true,

            // Any other transition is invalid
            _ => false,
        }
    }

    /// Attempt to transition to a new state, validating the transition
    pub fn transition_to(&self, new_state: State) -> Result<State, ZInitError> {
        if self.can_transition_to(&new_state) {
            Ok(new_state)
        } else {
            Err(ZInitError::invalid_state_transition(format!(
                "Invalid transition from {:?} to {:?}",
                self, new_state
            )))
        }
    }

    /// Check if the state is considered "active" (running or in progress)
    pub fn is_active(&self) -> bool {
        matches!(self, State::Running | State::Spawned)
    }

    /// Check if the state is considered "terminal" (success or failure)
    pub fn is_terminal(&self) -> bool {
        matches!(self, State::Success | State::Error(_) | State::Failure)
    }

    /// Check if the state is considered "successful"
    pub fn is_successful(&self) -> bool {
        matches!(self, State::Success | State::Running)
    }

    /// Check if the state is considered "failed"
    pub fn is_failed(&self) -> bool {
        matches!(self, State::Error(_) | State::Failure | State::TestFailure)
    }
}
