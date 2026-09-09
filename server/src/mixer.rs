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

//! Owning the ACC2 sound card's mixer state.
//!
//! # Why the server has to own this
//!
//! The card's mixer settings are load-bearing on this interface: capture
//! gain sets the ACC2 receive level, AGC has to be off for anything
//! amplitude-linear, and the mic monitor has to be off or it loops back.
//! Those three are asserted and re-asserted.
//!
//! The playback level -- TX drive -- is **reported and never set**. It
//! belongs to whatever generates the transmit audio. See
//! [`report_playback`] for why that distinction is not cosmetic.
//!
//! **Every USB re-enumeration reverts all of it.** ALSA restores its own
//! saved values while PipeWire goes on reporting 100%, so nothing looks
//! wrong from the desktop while the drive has silently dropped 20 dB. On
//! 2026-09-07 that presented as "TX ALC worked, then stopped" with nothing
//! changed, on a dongle that reached device number 113 on one bus in a
//! single evening. See the bench record, item 33.
//!
//! # Mixer access is not PCM access
//!
//! These are separate ALSA devices, and that is the point: PipeWire or
//! WSJT-X will usually hold the PCM stream, and the server still has to be
//! able to set the mixer. Nothing here opens or needs the PCM.
//!
//! # Finding the card
//!
//! By **name**, never by index. The index is exactly what a re-enumeration
//! changes, which is the event this exists to survive.

use alsa::mixer::{Mixer, SelemChannelId};
use tracing::{info, warn};

/// The values an operator can set. Everything else here is an invariant.
///
/// Levels are the card's own raw units rather than dB: they are what
/// `amixer` prints, what a bench note records, and they avoid a rounding
/// argument with a card whose dB scale is its own idea. The asserted value
/// is logged with the dB the card reports for it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MixerSettings {
    /// Capture gain. `None` leaves it alone.
    ///
    /// Station-specific and therefore a flag: 0 is correct on the bench
    /// this was written for, where the item 26 baseline had been taken at
    /// +23.8 dB — 24 dB hot.
    pub capture: Option<i64>,
    /// Playback level, which sets TX drive. `None` leaves it alone.
    pub playback: Option<i64>,
}

impl MixerSettings {
    /// Whether there is anything to do beyond asserting the invariants.
    pub fn is_empty(&self) -> bool {
        self.capture.is_none() && self.playback.is_none()
    }
}

/// Controls that are invariants rather than preferences.
///
/// AGC compresses block-to-block variation, which is wrong for any
/// amplitude-linear use; the mic monitor feeds the input back to the
/// output. Neither is a station's choice, so neither is a flag.
const AGC: &str = "Auto Gain Control";

/// Assert the mixer state for the card behind `spec`, and say what it did.
///
/// Errors are returned rather than logged here so the caller can decide
/// how loud to be: on a first open a missing card is worth a warning, and
/// on a retry it is the ordinary state of a dongle that has not come back
/// yet.
pub fn assert_state(spec: &str, settings: &MixerSettings) -> Result<Vec<String>, String> {
    let card = find_card(spec)?;
    let mixer = Mixer::new(&format!("hw:{card}"), false)
        .map_err(|e| format!("could not open the mixer on hw:{card}: {e}"))?;

    let mut did = Vec::new();

    for selem in mixer.iter().filter_map(alsa::mixer::Selem::new) {
        let name = selem.get_id().get_name().unwrap_or_default().to_string();

        // Capture has to be on, or the card is muted no matter what the
        // gain says.
        if selem.has_capture_switch() {
            let was = selem
                .get_capture_switch(SelemChannelId::mono())
                .unwrap_or(0);
            if was == 0 {
                let _ = selem.set_capture_switch_all(1);
                did.push(format!("{name}: capture switch was OFF, turned on"));
            }
        }

        // AGC off. Measured inert on this card for level, but it does
        // compress, and a data path wants neither.
        if name == AGC && selem.has_playback_switch() {
            let was = selem
                .get_playback_switch(SelemChannelId::mono())
                .unwrap_or(0);
            if was != 0 {
                let _ = selem.set_playback_switch_all(0);
                did.push(format!("{name}: was ON, turned off"));
            }
        }

        // The mic monitor loops input straight back to the output. Off,
        // always -- but not the Speaker, which is the TX drive path.
        if name != AGC && name != "Speaker" && selem.has_playback_switch() {
            let was = selem
                .get_playback_switch(SelemChannelId::mono())
                .unwrap_or(0);
            if was != 0 {
                let _ = selem.set_playback_switch_all(0);
                did.push(format!("{name}: monitor/playback was ON, turned off"));
            }
        }

        if let Some(want) = settings.capture {
            if selem.has_capture_volume() {
                did.extend(set_capture(&selem, &name, want));
            }
        }
        // Reported, not asserted -- see `report_playback`.
        if let Some(expected) = settings.playback {
            if selem.has_playback_volume() && name == "Speaker" {
                did.extend(report_playback(&selem, &name, expected));
            }
        }
    }

    // Deliberately empty when nothing needed changing. The caller decides
    // whether "nothing to do" is worth a line; on the retry path it runs
    // repeatedly and saying so every time would bury the one message that
    // matters -- the moment a re-enumeration reverted something.
    Ok(did)
}

fn set_capture(selem: &alsa::mixer::Selem, name: &str, want: i64) -> Vec<String> {
    let (min, max) = selem.get_capture_volume_range();
    let want = want.clamp(min, max);
    let was = selem
        .get_capture_volume(SelemChannelId::mono())
        .unwrap_or(-1);
    if was == want {
        return Vec::new();
    }
    match selem.set_capture_volume_all(want) {
        Ok(()) => vec![format!(
            "{name}: capture gain {was} -> {want} (range {min}..{max}{})",
            db_suffix(selem.ask_capture_vol_db(want))
        )],
        Err(e) => vec![format!("{name}: could not set capture gain to {want}: {e}")],
    }
}

/// Report a TX drive level that has moved. Never set it.
///
/// # Why this one reports where the others correct
///
/// The playback level is the transmit drive, and the transmit audio
/// belongs to whatever is generating it -- WSJT-X, fldigi, an operator
/// with a mixer open. Asserting it takes that away: the level an operator
/// chose is overwritten on the next re-assertion, and they get no say.
///
/// This *did* assert it, from `ba11e21` on 2026-09-07 until 2026-09-09.
/// The station ran with `--acc2-playback 151`, which is the maximum of
/// that card's 0..151 range, so the server pinned transmit drive at full
/// scale and put it back every time anything moved it.
///
/// Item 33's finding stands and is the reason this function still exists:
/// a USB re-enumeration reverts the card's mixer while PipeWire goes on
/// reporting 100%, so drive silently drops 20 dB with nothing looking
/// wrong from the desktop. But **the detection was the valuable half, not
/// the correction**. Saying "your transmit drive changed and here is what
/// it changed to" solves that without deciding what the level should be.
///
/// The capture gain, the AGC switch and the mic monitor keep asserting.
/// Those are invariants of this interface rather than anybody's levels --
/// nothing else manages them, AGC has to be off for anything
/// amplitude-linear, and the monitor loops back if it is on.
fn report_playback(selem: &alsa::mixer::Selem, name: &str, expected: i64) -> Vec<String> {
    let (min, max) = selem.get_playback_volume_range();
    let expected = expected.clamp(min, max);
    let now = selem
        .get_playback_volume(SelemChannelId::mono())
        .unwrap_or(-1);
    if now == expected {
        return Vec::new();
    }
    vec![format!(
        "{name}: TX drive is {now}, not the {expected} this station expects \
         (range {min}..{max}{}). NOT changed -- transmit audio belongs to \
         whatever generates it. Set it where you set it before, or pass a \
         different --acc2-playback if {now} is now correct.",
        db_suffix(selem.ask_playback_vol_db(now))
    )]
}

fn db_suffix(v: Result<alsa::mixer::MilliBel, alsa::Error>) -> String {
    match v {
        Ok(mb) => format!(", {:.2} dB", mb.to_db()),
        Err(_) => String::new(),
    }
}

/// Find the ALSA card index for an `--acc2-audio` spec.
///
/// Matched on the card's own name, never its index: the index is precisely
/// what a re-enumeration changes, and surviving that is the whole reason
/// this module exists.
fn find_card(spec: &str) -> Result<i32, String> {
    // `audio:USB PnP Sound Device` -- the part after the scheme is what a
    // person recognises and what ALSA also calls it.
    let wanted = spec
        .rsplit(':')
        .next()
        .unwrap_or(spec)
        .trim()
        .to_lowercase();
    if wanted.is_empty() {
        return Err("no device name to match a sound card against".to_string());
    }

    let mut seen = Vec::new();
    for card in alsa::card::Iter::new().flatten() {
        let index = card.get_index();
        let name = card.get_name().unwrap_or_default();
        let long = card.get_longname().unwrap_or_default();
        seen.push(format!("hw:{index} {name}"));
        if name.to_lowercase().contains(&wanted) || long.to_lowercase().contains(&wanted) {
            return Ok(index);
        }
    }
    Err(format!(
        "no sound card matching {wanted:?}; saw [{}]",
        seen.join(", ")
    ))
}

/// Assert and log, at the volume the occasion deserves.
///
/// A correction is always logged: that single line is what would have
/// answered the 2026-09-07 "ALC worked, then stopped" question in seconds
/// instead of an evening. Finding nothing to do is logged only on the
/// first pass, because the retry path runs every couple of seconds
/// forever and a heartbeat saying "still fine" is how a log stops being
/// read.
pub fn assert_and_log(spec: &str, settings: &MixerSettings, first: bool) {
    match assert_state(spec, settings) {
        Ok(lines) if lines.is_empty() => {
            if first {
                info!("ACC2 mixer: {spec}: already as configured");
            }
        }
        Ok(lines) => {
            for line in lines {
                info!("ACC2 mixer: {line}");
            }
        }
        // On a retry this is the ordinary state of a dongle that has not
        // come back yet, and warning every two seconds would bury the log.
        Err(e) if !first => info!("ACC2 mixer: {e}"),
        Err(e) => warn!("ACC2 mixer: {e}"),
    }
}

#[cfg(test)]
mod tx_drive_is_not_ours {
    use super::*;

    #[test]
    fn the_settings_type_still_carries_a_playback_expectation() {
        // The flag survives the change of meaning: the station still says
        // what it expects TX drive to be, and the server still notices
        // when it moves. What changed is that noticing no longer means
        // overwriting.
        let s = MixerSettings {
            capture: Some(0),
            playback: Some(151),
        };
        assert!(!s.is_empty());
    }

    #[test]
    fn nothing_in_this_module_writes_a_playback_volume() {
        // The assertion that actually holds the rule, checked against the
        // source rather than a mock: an ALSA `Selem` cannot be constructed
        // without a card, so a behavioural test here would need the
        // hardware, and a test that needs the operator's radio to run is a
        // test that does not run.
        //
        // `set_playback_volume_all` is the only call that can move TX
        // drive. If it comes back, this fails and the reviewer is pointed
        // at `report_playback`'s doc comment for why it must not.
        // Only the real code: the test module below names the call in
        // order to look for it, and must not match itself.
        let src = include_str!("mixer.rs");
        let production = src.split("#[cfg(test)]").next().unwrap_or(src);
        let calls: Vec<&str> = production
            .lines()
            .filter(|l| l.contains("set_playback_volume_all"))
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect();
        assert!(
            calls.is_empty(),
            "this module sets a playback volume again: {calls:?}. TX drive \
             belongs to whatever generates the transmit audio -- see \
             `report_playback`."
        );
    }

    #[test]
    fn the_switch_invariants_are_still_asserted() {
        // The other half of the split, so a later reader does not "tidy"
        // the whole module into report-only. AGC off and monitor off are
        // invariants of this interface, not anybody's levels: nothing else
        // manages them, and the monitor loops transmit audio back if on.
        let src = include_str!("mixer.rs");
        let src = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert!(
            src.contains("set_playback_switch_all(0)"),
            "the AGC/monitor switches must still be asserted"
        );
        assert!(
            src.contains("set_capture_volume_all"),
            "the capture gain must still be asserted"
        );
    }
}
