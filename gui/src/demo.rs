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

//! Capability sets to render against, without a radio or a socket.
//!
//! Not `#[cfg(test)]`: `examples/render.rs` needs these too, and a still
//! of the console showing a disconnected state says nothing about whether
//! the layout is right.
//!
//! These are `CapabilitiesWire` — the shape that arrives from a server —
//! rather than the `radio` crate's declaration, because that is what this
//! crate can see. `gui` never depends on `radio`: it is network-only, and
//! the radio on the other end might not be a TS-570D at all.
//!
//! `caps_bare` is the more useful of the two. A console tested only
//! against a well-equipped radio quietly grows assumptions that it has a
//! memory, a menu, a spectrum source — and then draws a control that can
//! never work the first time somebody points it at something simpler.

use cat_native::{
    CapabilitiesWire, FilterWire, FrequencyRange, Installation, SignalSupport, VfoCapability,
};

// The TS-570D's capabilities were transcribed here by hand, and the copy
// drifted: it listed four meters where the radio declares five, so the
// still was missing the compression meter -- the same way the FT-991A's
// copy was missing VDD and COMP.
//
// They now come straight from `radio::capabilities::TS570D`, in
// `examples/render.rs`. That is the only place that can reach them: this
// crate depends on `radio` as a **dev**-dependency only, because the
// binary stays protocol-only (ADR 0008 §3), and a fixture that cannot be
// derived is a fixture that will drift again.

/// A radio with nothing but a dial.
///
/// No memory, no menu, no spectrum, no shift, no split. Every panel has to
/// survive this, and the console has to say what is missing rather than
/// offer a control that cannot work.
pub fn bare() -> CapabilitiesWire {
    CapabilitiesWire {
        model: "Bare Radio".to_string(),
        endpoints: Vec::new(),
        vfos: VfoCapability {
            count: 1,
            split: false,
            rit_hz: None,
            xit_hz: None,
        },
        modes: Vec::new(),
        tuning_steps_hz: Vec::new(),
        rx_range: FrequencyRange::new(1_800_000, 30_000_000),
        filters: FilterWire {
            if_shift_hz: None,
            widths_hz: None,
            notch: false,
        },
        meters: Vec::new(),
        memory: None,
        menu: None,
        signal: SignalSupport::None,
        // A fixture for a still, not a server: the arrangement is the
        // server's to author, so a console drawn from this uses its own
        // default.
        layout: None,
        theme: None,
        installation: Installation::default(),
    }
}
