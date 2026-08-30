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

//! The TS-570D GPU console.
//!
//! Per ADR 0008 this crate owns **this radio's layout, feature set,
//! keybindings and visual identity, and nothing else**. The waterfall
//! pass, meter scaling, S-unit formatting and settings rendering come from
//! `cat-ui` and `cat-ui-egui`; the wire comes from `cat-native`. There is
//! no dependency on `radio` or on any transport: the console is
//! network-only, and the thing on the other end might not even be a
//! TS-570D.
//!
//! # Where the logic lives, and why
//!
//! Everything with a decision in it — which workspaces exist, where a
//! click tunes to, what a typed command means, which quick controls this
//! radio has — is a plain function over a `CapabilitiesWire`, tested
//! without a window. What is left in the egui code is placement.
//!
//! That split is not tidiness. A GUI's rendering is the part hardest to
//! assert on and easiest to eyeball; its behaviour is the reverse. Putting
//! the frequency mapping in the draw call would make the one thing that
//! can be *wrong* the one thing nothing can test.

pub mod app;
pub mod command;
pub mod quick;
pub mod readout;
pub mod theme;
pub mod tuning;
pub mod workspace;

mod test_support;

pub use command::{Action, ParseError};
pub use quick::Control;
pub use workspace::{Tab, TabEntry};
