pub mod blob_store;
#[cfg(target_os = "macos")]
mod ffi;
#[cfg(target_os = "macos")]
mod formats;
pub mod import;
#[cfg(target_os = "macos")]
mod macos;
pub mod models;
#[cfg(target_os = "macos")]
mod native_callbacks;
pub mod repository;
pub mod settings;
#[cfg(target_os = "macos")]
mod worker;

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreError {
    InvalidInput,
    Storage,
    MissingContent,
    Unsupported,
    Busy,
    Closed,
}

impl fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidInput => "invalid_input",
            Self::Storage => "storage_unavailable",
            Self::MissingContent => "content_unavailable",
            Self::Unsupported => "unsupported",
            Self::Busy => "busy",
            Self::Closed => "closed",
        })
    }
}

impl std::error::Error for CoreError {}

impl From<rusqlite::Error> for CoreError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Storage
    }
}

impl From<std::io::Error> for CoreError {
    fn from(_: std::io::Error) -> Self {
        Self::Storage
    }
}

impl From<serde_json::Error> for CoreError {
    fn from(_: serde_json::Error) -> Self {
        Self::InvalidInput
    }
}

pub type Result<T> = std::result::Result<T, CoreError>;
