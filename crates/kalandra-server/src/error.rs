//! Server error types.

use std::fmt;

use crate::server_error::ServerError as DriverError;

/// Errors that can occur in the server.
#[derive(Debug)]
pub enum ServerError {
    /// Configuration error
    Config(String),

    /// Transport/network error
    Transport(String),

    /// Protocol error
    Protocol(String),

    /// Internal error
    Internal(String),

    /// Driver error
    Driver(DriverError),
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(msg) => write!(f, "configuration error: {}", msg),
            Self::Transport(msg) => write!(f, "transport error: {}", msg),
            Self::Protocol(msg) => write!(f, "protocol error: {}", msg),
            Self::Internal(msg) => write!(f, "internal error: {}", msg),
            Self::Driver(err) => write!(f, "driver error: {}", err),
        }
    }
}

impl std::error::Error for ServerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Driver(err) => Some(err),
            _ => None,
        }
    }
}

impl From<DriverError> for ServerError {
    fn from(err: DriverError) -> Self {
        Self::Driver(err)
    }
}

impl From<std::io::Error> for ServerError {
    fn from(err: std::io::Error) -> Self {
        Self::Transport(err.to_string())
    }
}
