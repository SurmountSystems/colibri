//! Crate-wide error type.

use std::io;
use std::path::PathBuf;

/// Errors produced by colibri-sys host APIs.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("invalid argument: {0}")]
    Invalid(String),

    #[error("model path error at {path}: {message}")]
    Model { path: PathBuf, message: String },

    #[error("plan error: {0}")]
    Plan(String),

    #[error("engine error: {0}")]
    Engine(String),

    #[error("serve protocol error: {0}")]
    Protocol(String),

    #[error("doctor check failed: {0}")]
    Doctor(String),

    #[error("install error: {0}")]
    Install(String),

    #[error("feature not enabled: {0}")]
    FeatureDisabled(&'static str),

    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::Invalid(msg.into())
    }

    pub fn model(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::Model {
            path: path.into(),
            message: message.into(),
        }
    }

    pub fn protocol(msg: impl Into<String>) -> Self {
        Self::Protocol(msg.into())
    }

    pub fn engine(msg: impl Into<String>) -> Self {
        Self::Engine(msg.into())
    }
}

/// Result alias for colibri-sys.
pub type Result<T> = std::result::Result<T, Error>;
