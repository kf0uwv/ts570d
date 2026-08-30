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

//! This console's visual identity.
//!
//! ADR 0011 leaves visual identity in the app: `cat-ui-egui` draws the
//! structure and each console chooses the colours. These are the values
//! from the accepted mockup (`planning/designer/mockups/console/`,
//! option 3), kept in one place so a change is a change to the design
//! rather than to nine call sites.

use egui::Color32;

pub const BG: Color32 = Color32::from_rgb(0x06, 0x08, 0x0b);
pub const PANEL: Color32 = Color32::from_rgb(0x0d, 0x11, 0x15);
pub const PANEL_ALT: Color32 = Color32::from_rgb(0x0a, 0x0d, 0x10);
pub const LINE: Color32 = Color32::from_rgb(0x1c, 0x24, 0x2a);
pub const LINE_BRIGHT: Color32 = Color32::from_rgb(0x29, 0x33, 0x3b);

pub const TEXT: Color32 = Color32::from_rgb(0xc6, 0xd3, 0xdc);
pub const DIM: Color32 = Color32::from_rgb(0x6e, 0x7f, 0x8c);
pub const DIMMER: Color32 = Color32::from_rgb(0x46, 0x53, 0x5d);

pub const AMBER: Color32 = Color32::from_rgb(0xe6, 0xab, 0x44);
pub const GREEN: Color32 = Color32::from_rgb(0x4e, 0xc9, 0x7e);
pub const RED: Color32 = Color32::from_rgb(0xe2, 0x54, 0x3c);
pub const BLUE: Color32 = Color32::from_rgb(0x5a, 0xa6, 0xd8);
pub const VIOLET: Color32 = Color32::from_rgb(0x9b, 0x7f, 0xd4);

pub const SELECT: Color32 = Color32::from_rgb(0x1c, 0x4d, 0x6b);
pub const SELECT_LINE: Color32 = Color32::from_rgb(0x4d, 0x94, 0xc0);

/// The colour a capability that is absent is drawn in.
///
/// Absent is not the same as off, and the console says so in one colour
/// consistently: an absent capability is `DIMMER` and unreachable, an
/// inactive one keeps its normal weight. Collapsing the two is how an
/// operator ends up hunting for a control that was never going to exist.
pub const ABSENT: Color32 = DIMMER;

/// Apply the console's identity to an egui context.
pub fn install(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = PANEL_ALT;
    visuals.override_text_color = Some(TEXT);
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, LINE);
    visuals.widgets.inactive.bg_fill = PANEL;
    visuals.widgets.hovered.bg_fill = SELECT;
    visuals.widgets.active.bg_fill = SELECT;
    visuals.selection.bg_fill = SELECT;
    visuals.selection.stroke = egui::Stroke::new(1.0, SELECT_LINE);
    ctx.set_visuals(visuals);
}
