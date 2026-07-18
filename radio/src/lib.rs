// Copyright 2026 Matt Franklin
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Kenwood TS-570D CAT Protocol Implementation
//!
//! This crate provides a complete implementation of the Kenwood TS-570D
//! Computer Aided Transceiver (CAT) protocol. The implementation is data-driven,
//! with a comprehensive command table that defines all supported commands and
//! their metadata.
//!
//! # Architecture
//!
//! - `ts570d`: Typed [`Ts570d`] client, wrapping `cat_client::CatClient` for sending
//!   commands and reading responses
//! - `ts570d_radio`: The single [`TS570D_COMMAND_TABLE`] and the `Ts570dRadio` state machine
//! - `radio_trait`: Controller/UI-facing `Radio` trait + domain types (`Mode`, `Frequency`, ...)
//! - `protocol`: Typed TS-570D response parsing
//!
//! # Usage
//!
//! ```no_run
//! use radio::TS570D_COMMAND_TABLE;
//!
//! // Look up a command definition in the single command table.
//! let fa = TS570D_COMMAND_TABLE.find("FA").unwrap();
//! assert!(fa.is_readable());
//! assert!(fa.is_writable());
//! ```

pub mod protocol;
pub mod radio_trait;
pub mod ts570d;
pub mod ts570d_radio;

mod logger {
    pub type StateChange = crate::ts570d_radio::Ts570dEvent;
}

mod radio_state {
    pub use crate::ts570d_radio::MemoryChannel;
    pub type RadioState = crate::ts570d_radio::Ts570dState;
}

mod ts570d_radio_handlers;

pub use protocol::{Response, ResponseFramer, ResponseParser};
pub use radio_trait::{
    Frequency, InformationResponse, MemoryChannelEntry, Mode, NopRadio, Radio, RadioError,
    RadioResult,
};
pub use ts570d::Ts570d;
pub use ts570d_radio::{
    Ts570dCommandId, Ts570dEvent, Ts570dRadio, Ts570dState, TS570D_COMMAND_TABLE,
};
