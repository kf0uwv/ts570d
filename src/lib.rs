//! TS-570D Radio Control Application
//!
//! Main application that coordinates all workspace crates.
//! This crate provides framework-level coordination and bootstrapping.

// Re-export framework for other crates to use
pub extern crate framework;

// Re-export workspace crates for easy access
pub extern crate emulator;
pub extern crate radio;
pub extern crate serial;
pub extern crate ui;

// Framework coordination structures
pub mod app;

// Re-export common framework types
pub use app::*;
