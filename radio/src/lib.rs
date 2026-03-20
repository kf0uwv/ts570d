//! Kenwood TS-570D CAT Protocol Implementation
//!
//! This crate provides a complete implementation of the Kenwood TS-570D
//! Computer Aided Transceiver (CAT) protocol. The implementation is data-driven,
//! with a comprehensive command table that defines all supported commands and
//! their metadata.
//!
//! # Architecture
//!
//! - `commands`: Command table with metadata for all TS-570D CAT commands
//!
//! # Usage
//!
//! ```no_run
//! use radio::commands::CommandMetadata;
//!
//! // Look up command metadata
//! let fa_cmd = CommandMetadata::find("FA").unwrap();
//! assert!(fa_cmd.supports_read);
//! assert!(fa_cmd.supports_write);
//! ```

pub mod commands;
