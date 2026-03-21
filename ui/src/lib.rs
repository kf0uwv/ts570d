//! TS-570D Terminal UI
//!
//! Provides the ratatui-based terminal interface for the radio controller.

pub mod layout;
pub mod terminal;

pub use terminal::run_ui;

#[derive(Debug, thiserror::Error)]
pub enum UiError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type UiResult<T> = Result<T, UiError>;
