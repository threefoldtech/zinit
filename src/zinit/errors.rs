use thiserror::Error;

/// Errors that can occur in the zinit module
#[derive(Error, Debug)]
pub enum ZInitError {
    /// Service name is unknown
    #[error("service name {name:?} unknown")]
    UnknownService { name: String },

    /// Service is already being monitored
    #[error("service {name:?} already monitored")]
    ServiceAlreadyMonitored { name: String },

    /// Service is up and running
    #[error("service {name:?} is up")]
    ServiceIsUp { name: String },

    /// Service is down and not running
    #[error("service {name:?} is down")]
    ServiceIsDown { name: String },

    /// Zinit is shutting down
    #[error("zinit is shutting down")]
    ShuttingDown,

    /// Invalid state transition
    #[error("service state transition error: {message}")]
    InvalidStateTransition { message: String },

    /// Dependency error
    #[error("dependency error: {message}")]
    DependencyError { message: String },

    /// Process error
    #[error("process error: {message}")]
    ProcessError { message: String },
}

impl ZInitError {
    /// Create a new UnknownService error
    pub fn unknown_service<S: Into<String>>(name: S) -> Self {
        ZInitError::UnknownService { name: name.into() }
    }

    /// Create a new ServiceAlreadyMonitored error
    pub fn service_already_monitored<S: Into<String>>(name: S) -> Self {
        ZInitError::ServiceAlreadyMonitored { name: name.into() }
    }

    /// Create a new ServiceIsUp error
    pub fn service_is_up<S: Into<String>>(name: S) -> Self {
        ZInitError::ServiceIsUp { name: name.into() }
    }

    /// Create a new ServiceIsDown error
    pub fn service_is_down<S: Into<String>>(name: S) -> Self {
        ZInitError::ServiceIsDown { name: name.into() }
    }

    /// Create a new InvalidStateTransition error
    pub fn invalid_state_transition<S: Into<String>>(message: S) -> Self {
        ZInitError::InvalidStateTransition { message: message.into() }
    }

    /// Create a new DependencyError error
    pub fn dependency_error<S: Into<String>>(message: S) -> Self {
        ZInitError::DependencyError { message: message.into() }
    }

    /// Create a new ProcessError error
    pub fn process_error<S: Into<String>>(message: S) -> Self {
        ZInitError::ProcessError { message: message.into() }
    }
}