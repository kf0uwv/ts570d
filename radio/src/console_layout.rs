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

//! What a TS-570D's console looks like.
//!
//! Authored here, not derived. A capability set says this radio has an IF
//! tap, four meters and a 52-item menu; it does not say that the waterfall
//! should dominate the screen, and no amount of inference will make it
//! say so. That judgement belongs to whoever knows the rig, which is this
//! crate.
//!
//! The parts are shared — `cat_layout::PanelKind` is a closed vocabulary,
//! so this composes a console out of components that already work and does
//! not get to invent a fifth kind of meter. What is this radio's own is
//! which of them appear and how much room each gets.
//!
//! # Why this radio gets this arrangement
//!
//! A TS-570D on this bench has an SDR on its CN4 IF tap, and the whole
//! reason for that hardware is to see the band. So the spectrum takes the
//! content pane and the workspace sits under it, rather than the spectrum
//! being one tab among four.
//!
//! Both rails are fixed-width: the AF FFT is 20 cells because that is
//! 150 Hz per cell, and the levels rail is sized to its longest label. A
//! percentage would make an instrument's resolution depend on how big
//! somebody's terminal happens to be.

use cat_layout::{Child, LayoutSpec, Node, PanelKind, Rgb, Size, Theme};

/// The meter rail's width, and with it the AF panels beneath it.
const RAIL_W: u16 = 22;

/// The levels rail's width: the longest label plus its value.
const LEVELS_W: u16 = 26;

/// The console this radio asks for.
pub fn layout() -> LayoutSpec {
    LayoutSpec::new(Node::rows(vec![
        // The readout is the one thing an operator looks at without
        // meaning to, so it is at the top and it is always there.
        // Three rows: the tab bar rides with the readout, and the readout
        // itself is two so the pending value can sit under the confirmed
        // one.
        // Five: the tab bar plus the readout's two rows, with room for the
        // GPU console's taller strip. The two renderers draw the same
        // panel at different densities, and the layout has to fit the
        // roomier of them or one clips.
        Child::panel(Size::Fixed(5), PanelKind::Readout),
        // Six: BAND, MODE, and the two ribbon rows with their labels. A
        // layout that gave it fewer would clip the ribbon, and a clipped
        // ribbon looks like a radio with fewer controls than it has.
        Child::panel(Size::Fixed(6), PanelKind::QuickBar),
        Child::new(
            Size::Min(8),
            Node::columns(vec![
                Child::new(
                    Size::Fixed(RAIL_W),
                    Node::rows(vec![
                        // Fixed to what the roomier renderer needs. It
                        // was `Min(4)`, which made it absorb every spare
                        // row in the column: at 120x40 it took fifteen
                        // rows to draw five, and the AF panels it is
                        // supposed to sit directly above started ten
                        // blank rows further down.
                        Child::panel(
                            Size::Fixed(cat_layout::METER_RAIL_ROWS),
                            PanelKind::MeterRail,
                        ),
                        // The AF panels under the meters, because both
                        // answer "what is the receiver doing right now"
                        // and an operator reads them together.
                        //
                        // They take the slack the meters no longer hold.
                        // Six rows each was the minimum that fits a
                        // header and a trace; more is not padding, it is
                        // vertical resolution -- a taller scope resolves
                        // an envelope a six-row one flattens, and a
                        // taller FFT separates two tones a six-row one
                        // merges into one bar.
                        Child::panel(Size::Fill(1), PanelKind::AfScope),
                        Child::panel(Size::Fill(1), PanelKind::AfFft),
                    ]),
                ),
                Child::new(
                    Size::Min(20),
                    Node::rows(vec![
                        // This radio has a tap, so the band gets the room.
                        Child::panel(Size::Fill(3), PanelKind::Spectrum),
                        Child::panel(Size::Fill(2), PanelKind::Workspace),
                    ]),
                ),
                Child::panel(Size::Fixed(LEVELS_W), PanelKind::LevelsRail),
            ]),
        ),
        Child::panel(Size::Fixed(1), PanelKind::Status),
        Child::panel(Size::Fixed(1), PanelKind::CommandLine),
    ]))
}

/// What a TS-570D looks like.
///
/// This radio is a mid-nineties Kenwood: a charcoal-grey case and an
/// **amber-backlit LCD**. That amber is the radio's own colour — it is
/// what an operator's eye goes to on the physical front panel — so it is
/// the ink here, on the dark grey the case is.
///
/// It is a monochrome display, and the palette says so. The one place
/// colour appears is where the radio itself uses it: the signal trace, and
/// red for transmit. A console that painted this rig in six colours would
/// be prettier than the radio and harder to read alongside it.
pub fn theme() -> Theme {
    Theme {
        // The case: charcoal, slightly warm.
        background: Rgb::hex(0x14120f),
        // A panel against it, one step up.
        panel: Rgb::hex(0x1e1b16),
        // The LCD's amber. Everything an operator reads is this.
        ink: Rgb::hex(0xffb347),
        // A set value, brighter than the ink rather than a different hue:
        // that is how a segment display shows emphasis, having no second
        // colour to reach for.
        accent: Rgb::hex(0xfff0d0),
        // The trace. Kept amber-family so the spectrum reads as part of
        // the same instrument rather than as something bolted on.
        signal: Rgb::hex(0xffd88a),
        // Transmit. The one place this radio's own panel goes red.
        warning: Rgb::hex(0xe2543c),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cat_layout::Area;

    /// Every panel this layout places, at the design size.
    fn placed(w: u16, h: u16) -> Vec<(PanelKind, Area)> {
        let spec = layout();
        [
            PanelKind::Readout,
            PanelKind::QuickBar,
            PanelKind::MeterRail,
            PanelKind::AfScope,
            PanelKind::AfFft,
            PanelKind::Spectrum,
            PanelKind::Workspace,
            PanelKind::LevelsRail,
            PanelKind::Status,
            PanelKind::CommandLine,
        ]
        .into_iter()
        .filter_map(|k| spec.find(Area::new(0, 0, w, h), &k).map(|a| (k, a)))
        .collect()
    }

    #[test]
    fn show_the_layout() {
        // Not an assertion -- a way to see the arrangement without a
        // terminal. `cargo test -p radio show_the_layout -- --nocapture`.
        for (k, a) in placed(120, 40) {
            println!(
                "{k:12?} cols {:>3}..{:<3} rows {:>3}..{:<3}  ({}x{})",
                a.x,
                a.x + a.width - 1,
                a.y,
                a.y + a.height - 1,
                a.width,
                a.height
            );
        }
    }

    #[test]
    fn no_two_panels_overlap() {
        // A panel drawn over another does not look like a layout bug; it
        // looks like the panel underneath is broken.
        let panels = placed(120, 40);
        for (i, (ka, a)) in panels.iter().enumerate() {
            for (kb, b) in &panels[i + 1..] {
                let apart = a.x + a.width <= b.x
                    || b.x + b.width <= a.x
                    || a.y + a.height <= b.y
                    || b.y + b.height <= a.y;
                assert!(apart, "{ka:?} at {a:?} overlaps {kb:?} at {b:?}");
            }
        }
    }

    #[test]
    fn every_row_and_column_is_claimed_by_something() {
        // Dead space is a layout that has quietly stopped adding up.
        let panels = placed(120, 40);
        let covered = |x: u16, y: u16| {
            panels
                .iter()
                .any(|(_, a)| x >= a.x && x < a.x + a.width && y >= a.y && y < a.y + a.height)
        };
        let mut gaps = Vec::new();
        for y in 0..40u16 {
            for x in 0..120u16 {
                if !covered(x, y) {
                    gaps.push((x, y));
                }
            }
        }
        assert!(
            gaps.is_empty(),
            "{} cells belong to no panel, first at {:?}",
            gaps.len(),
            gaps.first()
        );
    }

    #[test]
    fn this_radios_console_gives_the_band_the_room() {
        // The judgement this file exists to record: a radio with an IF tap
        // is a radio somebody bought an SDR for, and burying the waterfall
        // in a tab wastes it.
        let spec = layout();
        let spectrum = spec.find(Area::new(0, 0, 120, 40), &PanelKind::Spectrum);
        let workspace = spec.find(Area::new(0, 0, 120, 40), &PanelKind::Workspace);
        let (s, w) = (spectrum.unwrap(), workspace.unwrap());
        assert!(
            s.height > w.height,
            "spectrum {} vs workspace {}",
            s.height,
            w.height
        );
    }

    #[test]
    fn the_furniture_is_there() {
        // A console with no command line cannot be typed into, and the
        // failure is invisible until somebody presses `:`.
        let spec = layout();
        for f in [PanelKind::Status, PanelKind::CommandLine] {
            assert!(spec.root.places(&f), "{f:?} missing");
        }
    }

    #[test]
    fn the_af_panels_keep_the_width_their_resolution_needs() {
        // 20 cells is 150 Hz per cell. A layout that let them flex would
        // change what an operator is reading when they resize a window.
        let spec = layout();
        let area = Area::new(0, 0, 200, 60);
        for panel in [PanelKind::AfScope, PanelKind::AfFft] {
            assert_eq!(spec.find(area, &panel).unwrap().width, RAIL_W, "{panel:?}");
        }
    }

    #[test]
    fn it_still_draws_in_a_small_terminal() {
        // An 80x24 terminal is not exotic. What it loses is the rightmost
        // rail; what it keeps is the radio.
        let spec = layout();
        let placed = spec.resolve(Area::new(0, 0, 80, 24));
        for must in [
            PanelKind::Readout,
            PanelKind::Spectrum,
            PanelKind::CommandLine,
        ] {
            assert!(
                placed.iter().any(|p| p.kind == must),
                "{must:?} lost at 80x24"
            );
        }
    }
}
