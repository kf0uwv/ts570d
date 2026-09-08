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

//! A complete picture of a TS-570D's configuration, and why taking one
//! needs the operator's hand on the panel.
//!
//! # The constraint
//!
//! `EX;` reports only the menu the **front panel** has selected. CAT can
//! write any menu blind, but it cannot name a menu to read and it cannot
//! move the selection. Established on the physical radio 2026-09-07: with
//! the panel on menu 34, `EX0200008;` left `EX;` still answering
//! `EX0340009;` while `PT;` moved from `PT04;` to `PT08;` — the write
//! landed, the selection did not follow it.
//!
//! So a complete picture of this radio **cannot be obtained over CAT
//! alone**. Fifty-two menus, one readable at a time, and only the one the
//! operator is looking at.
//!
//! # What makes a tolerable capture possible
//!
//! `EX;` answers `EX<nn><vvvv>;` — it names the menu it is answering
//! *for*. Nothing here has to trust that the operator selected what it
//! asked for; it watches and records whatever arrives. That turns capture
//! from fifty-two prompts into one instruction — *sweep the MENU knob* —
//! with any order, backtracking and resuming all working for free.
//!
//! # Three classes of setting
//!
//! - **CAT-readable commands** — queried directly, no operator.
//! - **Panel-only menus** — the sweep.
//! - **Aliased menus** — a CAT command reports the same cell, so no panel
//!   step is needed *and* staleness can be detected later without one.
//!   Only menu 20 (`PT`) is proven; see [`MENU_ALIASES`].
//!
//! # What staleness this can and cannot see
//!
//! Class A and aliased menus can be re-read and diffed at any time. Every
//! other menu cannot — except the single one the panel happens to be
//! sitting on. [`DriftReport`] reports that gap explicitly rather than
//! saying "no drift found", which would be a claim about menus it never
//! looked at.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Menus are numbered 000–051 on this radio.
pub const MENU_COUNT: u8 = 52;

/// The current on-disk format. Bumped when a field changes meaning.
pub const SCHEMA: u32 = 1;

/// Menus a CAT command reports independently of the front panel.
///
/// Each entry is a **verified** identity, not a plausible one. A wrong
/// alias silently reports a value the radio never gave, and it would be
/// indistinguishable from a real reading in the file.
///
/// Menu 20 (CW RX pitch) == `PT` was verified on the physical radio
/// 2026-09-07 in both directions. Menu 35 (COM parameters) is implied by
/// the link working at all, but there is no command that reports it, so
/// it is not listed.
pub const MENU_ALIASES: &[(u8, &str)] = &[(20, "PT")];

/// The warning that has to reach the operator, in the file and on screen.
///
/// The whole feature exists because the radio cannot show this and
/// software cannot read it back. Stating it once, in one place, so the
/// file and the console cannot drift apart.
pub const RECALIBRATION_WARNING: &str = "\
This snapshot records menu values that CAT CANNOT READ BACK. If anyone changes a \
setting from the front panel after this capture, the menu section of this file is \
silently wrong and nothing in software can detect it. Re-run the calibration sweep \
after any front-panel menu change.";

/// How a menu value got into the snapshot.
///
/// Recorded per menu rather than assumed for the file, because the two
/// have different trust and different staleness: an aliased value can be
/// re-checked later without the operator, a swept one cannot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MenuSource {
    /// Read from `EX;` while the operator had this menu selected.
    Panel,
    /// Read from a CAT command that reports the same cell.
    Alias { command: String },
}

/// One menu's value, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuReading {
    pub value: u16,
    pub source: MenuSource,
}

/// Everything known about one radio's configuration at one moment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema: u32,
    /// ISO 8601 UTC, so the file is legible without a tool.
    pub captured_at: String,
    pub radio: String,
    /// Reads the operator should see the moment they open the file.
    pub warning: String,
    /// CAT-readable commands: code (`"FA"`) to payload (`"00014074000"`).
    pub settings: BTreeMap<String, String>,
    /// Menu number to its reading. Absent means never captured.
    pub menus: BTreeMap<u8, MenuReading>,
}

impl Snapshot {
    pub fn new(radio: &str, captured_at: String) -> Self {
        Self {
            schema: SCHEMA,
            captured_at,
            radio: radio.to_string(),
            warning: RECALIBRATION_WARNING.to_string(),
            settings: BTreeMap::new(),
            menus: BTreeMap::new(),
        }
    }

    pub fn record_setting(&mut self, code: &str, payload: &str) {
        self.settings.insert(code.to_string(), payload.to_string());
    }

    /// Record a menu reading. A later reading replaces an earlier one, so
    /// an operator who sweeps twice gets the newer value rather than a
    /// conflict.
    pub fn record_menu(&mut self, menu: u8, value: u16, source: MenuSource) {
        self.menus.insert(menu, MenuReading { value, source });
    }

    /// Menus never captured, ascending.
    ///
    /// Written into the file as well as offered here: a snapshot that
    /// silently omits menus reads as complete, and one that names its own
    /// gaps cannot be mistaken for a full picture.
    pub fn missing_menus(&self) -> Vec<u8> {
        (0..MENU_COUNT)
            .filter(|n| !self.menus.contains_key(n))
            .collect()
    }

    pub fn is_complete(&self) -> bool {
        self.missing_menus().is_empty()
    }

    /// Compare a stored snapshot against one just taken.
    ///
    /// `self` is the stored file, `current` what the radio says now.
    pub fn drift(&self, current: &Snapshot) -> DriftReport {
        let mut settings_changed = Vec::new();
        let mut operator_changed = Vec::new();
        for (code, was) in &self.settings {
            if let Some(now) = current.settings.get(code) {
                if now != was {
                    let change = Changed {
                        what: code.clone(),
                        was: was.clone(),
                        now: now.clone(),
                    };
                    if OPERATOR_CONTROLS.contains(&code.as_str()) {
                        operator_changed.push(change);
                    } else {
                        settings_changed.push(change);
                    }
                }
            }
        }

        let mut menus_changed = Vec::new();
        let mut menus_unchecked = Vec::new();
        for (menu, stored) in &self.menus {
            match current.menus.get(menu) {
                Some(now) if now.value != stored.value => menus_changed.push(Changed {
                    what: format!("menu {menu:03}"),
                    was: stored.value.to_string(),
                    now: now.value.to_string(),
                }),
                Some(_) => {}
                // Not that it matched — that nobody looked.
                None => menus_unchecked.push(*menu),
            }
        }

        DriftReport {
            settings_changed,
            operator_changed,
            menus_changed,
            menus_unchecked,
        }
    }
}

/// One setting that moved between two snapshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    pub what: String,
    pub was: String,
    pub now: String,
}

/// What a verify pass could and could not establish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriftReport {
    pub settings_changed: Vec<Changed>,
    /// Front-panel knobs that moved: see [`OPERATOR_CONTROLS`].
    ///
    /// Reported, never alarmed on. Somebody turning the AF gain is not
    /// evidence that the radio was reconfigured.
    pub operator_changed: Vec<Changed>,
    pub menus_changed: Vec<Changed>,
    /// Menus in the stored file that this pass never read.
    ///
    /// The honest half of the report. These are not "unchanged"; nothing
    /// looked at them, and on this radio nothing can without the operator
    /// selecting each one.
    pub menus_unchecked: Vec<u8>,
}

impl DriftReport {
    /// Something definitely moved.
    pub fn drifted(&self) -> bool {
        !self.settings_changed.is_empty() || !self.menus_changed.is_empty()
    }

    /// Whether this pass can honestly claim the radio still matches.
    ///
    /// Only when nothing drifted **and** every menu was actually read. An
    /// unchecked menu is not a passing menu.
    pub fn fully_verified(&self) -> bool {
        !self.drifted() && self.menus_unchecked.is_empty()
    }

    /// A changed CAT setting means somebody has been at the front panel,
    /// which makes every unreadable menu suspect even though none of them
    /// can be shown to have moved.
    ///
    /// Note this is deliberately *not* the same question as
    /// [`Self::fully_verified`]. A pass that simply did not sweep has
    /// established less, but it is not evidence that anything moved, and
    /// reporting it as though a recalibration were needed would make the
    /// warning fire when nothing has happened.
    pub fn recalibration_advised(&self) -> bool {
        self.drifted() || !self.menus_unchecked.is_empty()
    }
}

/// ISO 8601 UTC from a Unix timestamp.
///
/// Hand-rolled rather than pulling in a date crate for one string in one
/// file. Civil-from-days is Howard Hinnant's, which is the standard
/// derivation and correct across the proleptic Gregorian calendar.
pub fn iso8601_utc(unix_seconds: u64) -> String {
    let days = (unix_seconds / 86_400) as i64;
    let secs_of_day = unix_seconds % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// ---------------------------------------------------------------------------
// Driving a real radio
// ---------------------------------------------------------------------------

use crate::ts570d::Ts570d;
use crate::ts570d_radio::TS570D_COMMAND_TABLE;
use crate::RadioResult;
use cat_transport_core::{CatSession, TransportError};

/// Readings that are not settings, and must never enter a snapshot.
///
/// These change on their own between one read and the next — an S-meter
/// moves with the band. Capturing them would make every later verify
/// report drift, and an alarm that always fires is one nobody reads.
///
/// `IF` is excluded for a second reason as well: it is a composite that
/// includes the TX/RX bit, and its settings half is already covered by
/// `FA`, `MD` and friends.
///
/// `EX` is excluded for a third: its answer is not a setting at all but
/// *whichever menu the panel is on*, so recording it would report drift
/// every time somebody turned the knob. Worse, a restore would write it
/// back — and `EX<nnn><vvvv>;` is a menu write, so restoring a captured
/// `"EX": "0000000"` would silently set menu 0 to zero. Menu values have
/// their own section of the snapshot precisely so they are never confused
/// with this.
pub const VOLATILE: &[&str] = &["SM", "RM", "BY", "IF", "EX"];

/// Settings that are front-panel knobs, turned in the course of ordinary
/// operating.
///
/// Captured and restored like anything else — they are real state, and an
/// operator restoring a bench setup wants their gains back. But they are
/// **not evidence that anything was reconfigured**, and treating them as
/// such makes the drift warning fire every time somebody adjusts the
/// volume. The first run of this against the physical radio reported
/// "recalibrate, every menu is suspect" because the AF gain had moved
/// from 019 to 035, which is what an AF gain does.
///
/// `FA`/`FB` are here for the same reason and more strongly: the dial is
/// the most-turned control on any radio, and it moves on every QSO, every
/// band change and every click of a waterfall. A snapshot still records
/// where the radio was — that is worth having, and a restore should put
/// you back — but "the frequency changed" is not evidence that anyone
/// reconfigured anything. It fired on a 55 Hz difference.
///
/// Deliberately not `MD`: a mode change is a deliberate act that says
/// something about how the station is set up, and it is not continuously
/// variable. `SH`/`SL` (DSP slope) and `IS` (IF shift) are front-panel
/// knobs too, but are set for a band or a signal, so a change in one is
/// worth reporting.
pub const OPERATOR_CONTROLS: &[&str] = &["AG", "RG", "SQ", "FA", "FB"];

/// Settings a restore must never write back.
///
/// `PS0;` powers the radio off, which is not a setting being restored so
/// much as the session ending.
///
/// `AC` starts an antenna-tuner cycle, which **keys the PA at around 10 W
/// for up to 60 seconds**. Restoring a file must never put a transmitter
/// on the air on its own, whatever the antenna is connected to.
pub const NEVER_RESTORE: &[&str] = &["PS", "AC"];

/// Menus whose value changes what the radio physically does, listed so a
/// restore can call them out rather than burying them in fifty-two lines.
///
/// 38 and 39 are the two that produce a radio which looks broken with
/// nothing on the display to explain it.
pub const CONSEQUENTIAL_MENUS: &[(u8, &str)] = &[
    (33, "ACC2 AF input level"),
    (34, "ACC2 AF output level"),
    (38, "TX inhibit"),
    (39, "Linear amplifier control relay"),
];

/// Parse an `EX;` answer into the menu it is *for* and its value.
///
/// The reason a sweep can be a sweep: the radio names the menu, so nothing
/// has to trust that the operator selected the one it asked for.
pub fn parse_menu_answer(raw: &str) -> Option<(u8, u16)> {
    let body = raw.trim().strip_prefix("EX")?.strip_suffix(';')?;
    if body.len() != 7 {
        return None;
    }
    let menu: u8 = body[..3].parse().ok()?;
    let value: u16 = body[3..].parse().ok()?;
    (menu < MENU_COUNT).then_some((menu, value))
}

/// Strip a code and terminator from a query answer: `"PT04;"` -> `"04"`.
fn payload_of<'a>(raw: &'a str, code: &str) -> Option<&'a str> {
    raw.trim().strip_prefix(code)?.strip_suffix(';')
}

/// Every CAT-readable setting this radio actually answers.
///
/// Commands the radio rejects are skipped rather than recorded as errors:
/// the controller catalogue lists commands a TS-570D does not have, and a
/// snapshot should describe the radio in front of it.
pub async fn capture_settings<S>(
    radio: &mut Ts570d<S>,
    snapshot: &mut Snapshot,
) -> Result<usize, CaptureError>
where
    S: CatSession<Error = TransportError>,
{
    let mut captured = 0;
    // A command that never answers is not a dead link. This radio has ten
    // commands the controller catalogue lists and it does not implement;
    // most answer `?;`, but `MC` simply says nothing and the read times
    // out. Aborting on the first of those made a capture impossible.
    //
    // A dead link is still fatal, and is what a RUN of silences looks
    // like: three in a row and nothing after can be trusted. That is the
    // failure this abort was added for -- a stale handle answered every
    // command with an error and produced a confident five-setting file.
    let mut consecutive_silence = 0u32;
    for definition in TS570D_COMMAND_TABLE.definitions() {
        let code = definition.code;
        if !definition.readable || VOLATILE.contains(&code) {
            continue;
        }
        // Only the bare `CODE;` form. A query that needs a parameter is
        // asking a question this does not know how to pose.
        if !definition.query_forms.iter().any(|f| f.min_len == 0) {
            continue;
        }
        match classify(&radio.client.query(code).await, code) {
            Answer::Value(payload) => {
                consecutive_silence = 0;
                snapshot.record_setting(code, &payload);
                captured += 1;
            }
            Answer::NotSupported => consecutive_silence = 0,
            Answer::Unintelligible(answer) => {
                consecutive_silence += 1;
                if consecutive_silence >= MAX_CONSECUTIVE_SILENCE {
                    return Err(CaptureError {
                        code: code.to_string(),
                        answer,
                    });
                }
            }
        }
    }
    Ok(captured)
}

/// What one query answer actually was.
enum Answer {
    /// A well-formed reading for the command asked.
    Value(String),
    /// The radio refused: it does not have this command. Ordinary — the
    /// controller catalogue lists commands a TS-570D lacks.
    NotSupported,
    /// Anything else. A timeout, a transport error, or a frame for some
    /// other command.
    Unintelligible(String),
}

/// Sort a query result into the three things it can be.
///
/// The distinction that matters: a refusal is a fact about the radio and a
/// reason to move on, but an unintelligible answer means the link is not
/// carrying this conversation, and every reading after it is worthless.
/// Treating the second as the first is how a capture ends up writing a
/// confident-looking file with five settings in it.
fn classify<E: std::fmt::Display>(result: &Result<String, E>, code: &str) -> Answer {
    let raw = match result {
        Ok(raw) => raw.trim(),
        // A refused read is reported as an error by the client for
        // commands the table marks unreadable; nothing to record, and
        // nothing wrong with the link.
        Err(e) => return Answer::Unintelligible(e.to_string()),
    };
    if raw.is_empty() {
        return Answer::NotSupported;
    }
    if matches!(raw, "?;" | "E;" | "O;") {
        return Answer::NotSupported;
    }
    match payload_of(raw, code) {
        Some(payload) if !payload.is_empty() => Answer::Value(payload.to_string()),
        // A frame that is not for the command asked. Never silently
        // dropped: it means the stream is carrying somebody else's answer.
        _ => Answer::Unintelligible(raw.to_string()),
    }
}

/// Unanswered reads in a row that mean the link has gone, rather than a
/// command the radio does not implement.
pub const MAX_CONSECUTIVE_SILENCE: u32 = 3;

/// The link stopped carrying the conversation part-way through a capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureError {
    pub code: String,
    pub answer: String,
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the radio answered {:?} when asked for {} -- the link is not \
             carrying this conversation, so nothing after it can be trusted",
            self.answer, self.code
        )
    }
}

impl std::error::Error for CaptureError {}

/// Read the menus a CAT command reports, so the operator need not sweep
/// to them and so they stay checkable afterwards.
pub async fn capture_aliased_menus<S>(radio: &mut Ts570d<S>, snapshot: &mut Snapshot) -> usize
where
    S: CatSession<Error = TransportError>,
{
    let mut captured = 0;
    for (menu, code) in MENU_ALIASES {
        if let Answer::Value(payload) = classify(&radio.client.query(code).await, code) {
            if let Ok(value) = payload.parse::<u16>() {
                snapshot.record_menu(
                    *menu,
                    value,
                    MenuSource::Alias {
                        command: (*code).to_string(),
                    },
                );
                captured += 1;
            }
        }
    }
    captured
}

/// Read whichever menu the front panel currently has selected.
///
/// `Ok(None)` means the radio answered something that was not a menu —
/// a refusal, or a frame that did not survive. Not an error: during a
/// sweep this is polled continuously and a miss is ordinary.
pub async fn read_selected_menu<S>(radio: &mut Ts570d<S>) -> RadioResult<Option<(u8, u16)>>
where
    S: CatSession<Error = TransportError>,
{
    let raw = radio.client.query("EX").await?;
    Ok(parse_menu_answer(&raw))
}

/// Write one menu.
///
/// Blind by construction: there is no way to confirm it landed except to
/// have the operator select that menu and read it back, which is what the
/// verify sweep is for.
pub async fn write_menu<S>(radio: &mut Ts570d<S>, menu: u8, value: u16) -> RadioResult<()>
where
    S: CatSession<Error = TransportError>,
{
    radio
        .client
        .set("EX", format!("{menu:03}{value:04}"))
        .await?;
    Ok(())
}

/// What a restore did, and what it could not establish.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RestoreReport {
    /// CAT settings written and confirmed by reading them back.
    pub settings_confirmed: Vec<String>,
    /// CAT settings written whose read-back did not match.
    pub settings_failed: Vec<Changed>,
    /// Menus written. Every one is unconfirmed until a verify sweep.
    pub menus_written: Vec<u8>,
}

impl RestoreReport {
    /// Always true immediately after a restore.
    ///
    /// A restore is not finished when the writes are sent. Menu writes
    /// land silently and invisibly, so until the operator has swept the
    /// panel this has changed the radio without establishing anything.
    pub fn needs_verify_sweep(&self) -> bool {
        !self.menus_written.is_empty()
    }
}

/// Write a snapshot back to the radio.
///
/// Settings are written and read back. Menus are written blind, and the
/// caller **must** follow with a verify sweep — see
/// [`RestoreReport::needs_verify_sweep`].
pub async fn restore<S>(radio: &mut Ts570d<S>, snapshot: &Snapshot) -> RestoreReport
where
    S: CatSession<Error = TransportError>,
{
    let mut report = RestoreReport::default();

    for (code, want) in &snapshot.settings {
        if NEVER_RESTORE.contains(&code.as_str()) {
            continue;
        }
        let Some(definition) = TS570D_COMMAND_TABLE
            .definitions()
            .iter()
            .find(|d| d.code == code)
        else {
            continue;
        };
        if !definition.writable || definition.set_forms.is_empty() {
            continue;
        }
        // `definition.code` rather than the map key: the client's code
        // parameter is `&'static str`, and the definition is where the
        // static one lives.
        let static_code = definition.code;
        if radio.client.set(static_code, want).await.is_err() {
            continue;
        }
        match radio.client.query(static_code).await {
            Ok(raw) => {
                let got = payload_of(&raw, static_code).unwrap_or_default();
                if got == want {
                    report.settings_confirmed.push(code.clone());
                } else {
                    report.settings_failed.push(Changed {
                        what: code.clone(),
                        was: want.clone(),
                        now: got.to_string(),
                    });
                }
            }
            Err(_) => report.settings_failed.push(Changed {
                what: code.clone(),
                was: want.clone(),
                now: "<no answer>".to_string(),
            }),
        }
    }

    for (menu, reading) in &snapshot.menus {
        if write_menu(radio, *menu, reading.value).await.is_ok() {
            report.menus_written.push(*menu);
        }
    }

    report
}

/// What a startup calibration check found.
///
/// A separate type from [`DriftReport`] because the question is different:
/// drift asks "has this radio moved", and this asks "is there a usable
/// picture of this radio at all".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationStatus {
    /// No snapshot was configured. The menus are simply unknown.
    NotConfigured,
    /// A snapshot was named but could not be read.
    Unreadable { path: String, reason: String },
    /// A snapshot exists but does not cover every menu.
    Incomplete {
        path: String,
        captured: usize,
        missing: Vec<u8>,
    },
    /// The radio no longer matches the snapshot.
    Drifted { path: String, report: DriftReport },
    /// Complete, and everything checkable still matches.
    Current { path: String, captured_at: String },
    /// Only front-panel knobs have moved: see [`OPERATOR_CONTROLS`].
    ///
    /// Its own state rather than folded into `Drifted`, because the
    /// difference is the whole point — somebody turning the AF gain is
    /// ordinary operating, and warning about it trains people to ignore
    /// the warning that matters.
    OnlyKnobsMoved { path: String, report: DriftReport },
    /// The radio could not be asked. Not a calibration problem.
    CouldNotCheck { path: String, reason: String },
}

impl CalibrationStatus {
    /// Whether the operator should be told.
    ///
    /// Everything except a complete, matching snapshot. A radio whose menu
    /// settings nobody has recorded is the normal case, and still worth
    /// one line at startup: menus 38 and 39 alone can make a working radio
    /// look broken with nothing on the display to say why.
    pub fn is_warning(&self) -> bool {
        !matches!(
            self,
            CalibrationStatus::Current { .. } | CalibrationStatus::OnlyKnobsMoved { .. }
        )
    }

    /// What to print, most important first.
    pub fn lines(&self) -> Vec<String> {
        match self {
            CalibrationStatus::NotConfigured => vec![
                "No calibration snapshot: this radio's 52 menu settings are unknown.".to_string(),
                "CAT cannot read them back, so nothing here can tell you what they are."
                    .to_string(),
                "Capture one:  ts570d calibrate --port <port> --out <file>".to_string(),
                "Then start the server with  --calibration <file>".to_string(),
            ],
            CalibrationStatus::Unreadable { path, reason } => vec![
                format!("Calibration snapshot {path} could not be read: {reason}"),
                "The radio's menu settings are unknown.".to_string(),
            ],
            CalibrationStatus::Incomplete {
                path,
                captured,
                missing,
            } => vec![
                format!(
                    "Calibration snapshot {path} is INCOMPLETE: {captured} of {MENU_COUNT} \
                     menus captured, {} never read.",
                    missing.len()
                ),
                format!(
                    "Unknown menus: {}",
                    missing
                        .iter()
                        .map(|n| format!("{n:03}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                ),
                "Re-run the sweep to finish it:  ts570d calibrate --out <file>".to_string(),
            ],
            CalibrationStatus::Drifted { path, report } => {
                let mut lines = vec![format!(
                    "The radio no longer matches {path} — somebody has changed settings."
                )];
                for c in &report.settings_changed {
                    lines.push(format!("  {} was {} now {}", c.what, c.was, c.now));
                }
                for c in &report.operator_changed {
                    lines.push(format!(
                        "  {} was {} now {} (a front-panel knob; not counted as drift)",
                        c.what, c.was, c.now
                    ));
                }
                for c in &report.menus_changed {
                    lines.push(format!("  {} was {} now {}", c.what, c.was, c.now));
                }
                lines.push(
                    "Menu values cannot be read back over CAT, so every menu in that file \
                     is now suspect — not only the settings listed."
                        .to_string(),
                );
                lines.push("Recalibrate:  ts570d calibrate --out <file>".to_string());
                lines
            }
            CalibrationStatus::CouldNotCheck { path, reason } => vec![
                format!("Could not check the radio against {path}: {reason}"),
                "The snapshot may or may not still be accurate.".to_string(),
            ],
            CalibrationStatus::OnlyKnobsMoved { path, report } => {
                let mut lines = vec![format!("Calibration {path} matches.")];
                for c in &report.operator_changed {
                    lines.push(format!(
                        "  {} was {} now {} — a front-panel knob, not a reconfiguration",
                        c.what, c.was, c.now
                    ));
                }
                lines
            }
            CalibrationStatus::Current { path, captured_at } => {
                vec![format!(
                    "Calibration {path} (captured {captured_at}) matches."
                )]
            }
        }
    }
}

/// Compare a radio against a stored snapshot at startup.
///
/// `path` is `None` when no snapshot was configured, which is itself a
/// reportable state rather than a reason to skip the check.
pub async fn check<S>(radio: &mut Ts570d<S>, path: Option<&str>) -> CalibrationStatus
where
    S: CatSession<Error = TransportError>,
{
    let Some(path) = path else {
        return CalibrationStatus::NotConfigured;
    };
    let stored = match std::fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|text| serde_json::from_str::<Snapshot>(&text).map_err(|e| e.to_string()))
    {
        Ok(s) => s,
        Err(reason) => {
            return CalibrationStatus::Unreadable {
                path: path.to_string(),
                reason,
            }
        }
    };

    // Reported before any radio read: a file that never covered every menu
    // cannot become accurate by agreeing with the radio about the rest.
    let missing = stored.missing_menus();
    if !missing.is_empty() {
        return CalibrationStatus::Incomplete {
            path: path.to_string(),
            captured: stored.menus.len(),
            missing,
        };
    }

    let mut current = Snapshot::new(&stored.radio, String::new());
    if let Err(e) = capture_settings(radio, &mut current).await {
        return CalibrationStatus::CouldNotCheck {
            path: path.to_string(),
            reason: e.to_string(),
        };
    }
    capture_aliased_menus(radio, &mut current).await;

    let report = stored.drift(&current);
    if report.drifted() {
        CalibrationStatus::Drifted {
            path: path.to_string(),
            report,
        }
    } else if !report.operator_changed.is_empty() {
        CalibrationStatus::OnlyKnobsMoved {
            path: path.to_string(),
            report,
        }
    } else {
        CalibrationStatus::Current {
            path: path.to_string(),
            captured_at: stored.captured_at.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> Snapshot {
        Snapshot::new("TS-570D", "2026-09-07T15:00:00Z".to_string())
    }

    #[test]
    fn a_fresh_snapshot_is_missing_every_menu() {
        // Not "complete with 52 zeroes". A menu nobody has read is absent,
        // and the difference is the whole point of the file.
        let s = snap();
        assert_eq!(s.missing_menus().len(), MENU_COUNT as usize);
        assert!(!s.is_complete());
    }

    #[test]
    fn missing_menus_names_the_gaps_ascending() {
        let mut s = snap();
        for n in 0..MENU_COUNT {
            if n != 7 && n != 38 {
                s.record_menu(n, 0, MenuSource::Panel);
            }
        }
        assert_eq!(s.missing_menus(), vec![7, 38]);
        assert!(!s.is_complete());
    }

    #[test]
    fn a_snapshot_is_complete_only_with_all_fifty_two() {
        let mut s = snap();
        for n in 0..MENU_COUNT {
            s.record_menu(n, u16::from(n), MenuSource::Panel);
        }
        assert!(s.is_complete());
        assert!(s.missing_menus().is_empty());
    }

    #[test]
    fn a_second_reading_of_a_menu_replaces_the_first() {
        // An operator who sweeps twice gets the newer value, not a
        // conflict they have to resolve.
        let mut s = snap();
        s.record_menu(34, 4, MenuSource::Panel);
        s.record_menu(34, 9, MenuSource::Panel);
        assert_eq!(s.menus[&34].value, 9);
    }

    #[test]
    fn a_menu_records_how_it_was_read() {
        // An aliased value can be re-checked later without the operator; a
        // swept one cannot. The file has to keep them apart.
        let mut s = snap();
        s.record_menu(
            20,
            4,
            MenuSource::Alias {
                command: "PT".into(),
            },
        );
        s.record_menu(34, 9, MenuSource::Panel);
        assert_eq!(
            s.menus[&20].source,
            MenuSource::Alias {
                command: "PT".into()
            }
        );
        assert_eq!(s.menus[&34].source, MenuSource::Panel);
    }

    #[test]
    fn the_warning_travels_in_the_file() {
        // Whoever opens this months later must not have to already know
        // the EX; constraint to distrust the menu section.
        let s = snap();
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("CANNOT READ BACK"));
        assert!(json.contains("front panel"));
    }

    #[test]
    fn a_snapshot_round_trips_through_json() {
        let mut s = snap();
        s.record_setting("FA", "00014074000");
        s.record_setting("MD", "2");
        s.record_menu(
            20,
            4,
            MenuSource::Alias {
                command: "PT".into(),
            },
        );
        s.record_menu(38, 0, MenuSource::Panel);

        let json = serde_json::to_string_pretty(&s).unwrap();
        let back: Snapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.menus[&38].value, 0);
    }

    #[test]
    fn drift_spots_a_changed_cat_setting() {
        // `PA` (preamp), not `FA`: the dial is an operator control and
        // moves constantly, so it is reported without being called drift.
        let mut stored = snap();
        stored.record_setting("PA", "1");
        stored.record_setting("MD", "2");
        let mut now = snap();
        now.record_setting("PA", "0");
        now.record_setting("MD", "2");

        let d = stored.drift(&now);
        assert_eq!(d.settings_changed.len(), 1);
        assert_eq!(d.settings_changed[0].what, "PA");
        assert_eq!(d.settings_changed[0].was, "1");
        assert_eq!(d.settings_changed[0].now, "0");
        assert!(d.drifted());
    }

    #[test]
    fn drift_spots_a_changed_menu_when_one_was_actually_read() {
        let mut stored = snap();
        stored.record_menu(38, 1, MenuSource::Panel);
        let mut now = snap();
        now.record_menu(38, 0, MenuSource::Panel);

        let d = stored.drift(&now);
        assert_eq!(d.menus_changed.len(), 1);
        assert_eq!(d.menus_changed[0].what, "menu 038");
        assert!(d.drifted());
    }

    #[test]
    fn a_menu_nobody_read_is_reported_unchecked_not_unchanged() {
        // The crux of the whole type. A verify pass that only re-read the
        // CAT-readable settings has established NOTHING about the menus,
        // and must not be able to say otherwise.
        let mut stored = snap();
        stored.record_setting("FA", "00014074000");
        stored.record_menu(38, 1, MenuSource::Panel);
        stored.record_menu(39, 0, MenuSource::Panel);

        let mut now = snap();
        now.record_setting("FA", "00014074000");

        let d = stored.drift(&now);
        assert!(!d.drifted(), "nothing was shown to have moved");
        assert_eq!(d.menus_unchecked, vec![38, 39]);
        assert!(
            !d.fully_verified(),
            "two unread menus must not pass as verified"
        );
        assert!(d.recalibration_advised());
    }

    #[test]
    fn a_clean_full_pass_verifies() {
        let mut stored = snap();
        stored.record_setting("FA", "00014074000");
        for n in 0..MENU_COUNT {
            stored.record_menu(n, u16::from(n), MenuSource::Panel);
        }
        let now = stored.clone();

        let d = stored.drift(&now);
        assert!(!d.drifted());
        assert!(d.menus_unchecked.is_empty());
        assert!(d.fully_verified());
        assert!(!d.recalibration_advised());
    }

    #[test]
    fn a_changed_cat_setting_advises_recalibration_even_with_menus_intact() {
        // Somebody has had their hands on the front panel. That makes
        // every unreadable menu suspect, whatever the ones we can see say.
        let mut stored = snap();
        stored.record_setting("MD", "2");
        for n in 0..MENU_COUNT {
            stored.record_menu(n, 0, MenuSource::Panel);
        }
        let mut now = stored.clone();
        now.record_setting("MD", "3");

        let d = stored.drift(&now);
        assert!(d.drifted());
        assert!(d.recalibration_advised());
        assert!(!d.fully_verified());
    }

    #[test]
    fn every_alias_is_a_real_menu_number() {
        for (menu, command) in MENU_ALIASES {
            assert!(*menu < MENU_COUNT, "menu {menu} is out of range");
            assert_eq!(command.len(), 2, "{command} is not a CAT code");
        }
    }

    #[test]
    fn menu_twenty_is_the_cw_pitch() {
        // Verified on the physical radio 2026-09-07 in both directions.
        // Pinned so nobody drops the one alias that lets a menu be
        // re-checked without the operator.
        assert!(MENU_ALIASES.contains(&(20, "PT")));
    }

    #[test]
    fn a_menu_answer_names_the_menu_it_is_for() {
        // The property the whole sweep design rests on: the operator can
        // turn the knob in any order and the software still knows exactly
        // which cell it just read.
        assert_eq!(parse_menu_answer("EX0340009;"), Some((34, 9)));
        assert_eq!(parse_menu_answer("EX0000000;"), Some((0, 0)));
        assert_eq!(parse_menu_answer("EX0510012;"), Some((51, 12)));
    }

    #[test]
    fn a_menu_answer_that_is_not_one_is_refused() {
        for bad in [
            "?;",          // refusal
            "",            // silence
            "EX;",         // the query, echoed
            "EX034009;",   // one digit short
            "EX03400091;", // one too many
            "PT04;",       // a different command
            "EX0520000;",  // menu 52 does not exist
            "EXaaa0000;",  // not a number
        ] {
            assert_eq!(parse_menu_answer(bad), None, "should refuse {bad:?}");
        }
    }

    #[test]
    fn a_menu_answer_survives_surrounding_whitespace() {
        assert_eq!(parse_menu_answer("  EX0340009;\r\n"), Some((34, 9)));
    }

    #[test]
    fn a_refusal_is_not_a_reading_and_not_a_broken_link() {
        // The controller catalogue lists commands a TS-570D does not have.
        // Skipping them is right; aborting on them would make a capture
        // impossible.
        for refusal in ["?;", "E;", "O;", ""] {
            let r: Result<String, std::io::Error> = Ok(refusal.to_string());
            assert!(
                matches!(classify(&r, "FC"), Answer::NotSupported),
                "{refusal:?}"
            );
        }
    }

    #[test]
    fn a_well_formed_answer_yields_its_payload() {
        let r: Result<String, std::io::Error> = Ok("FA00014074000;".to_string());
        match classify(&r, "FA") {
            Answer::Value(v) => assert_eq!(v, "00014074000"),
            _ => panic!("should have read a value"),
        }
    }

    #[test]
    fn a_transport_failure_is_never_mistaken_for_a_reading() {
        // This is the bug this function exists to prevent. The broker
        // reports a timeout as a successful string beginning "ERR", and a
        // capture that treated it as an answer silently wrote a file with
        // five settings in it that looked entirely normal.
        let r: Result<String, std::io::Error> =
            Ok("ERR physical radio session did not respond within 5s".to_string());
        assert!(matches!(classify(&r, "FA"), Answer::Unintelligible(_)));
    }

    #[test]
    fn an_answer_for_a_different_command_is_never_recorded() {
        // A crossed stream. Recording it would file one command's value
        // under another's name, which is worse than no reading at all.
        let r: Result<String, std::io::Error> = Ok("MD2;".to_string());
        assert!(matches!(classify(&r, "FA"), Answer::Unintelligible(_)));
    }

    #[test]
    fn a_client_error_is_a_link_problem() {
        let r: Result<String, std::io::Error> =
            Err(std::io::Error::other("interrupted system call"));
        assert!(matches!(classify(&r, "FA"), Answer::Unintelligible(_)));
    }

    #[test]
    fn one_command_that_never_answers_is_not_a_dead_link() {
        // This radio does not implement `MC`, and rather than answering
        // `?;` it says nothing at all -- the read times out. Aborting on
        // the first such command made a capture impossible:
        //
        //   error: the radio answered "...Read timeout" when asked for MC
        //   No file written.
        assert!(
            MAX_CONSECUTIVE_SILENCE > 1,
            "a single unanswered command must not abort a capture"
        );
    }

    #[test]
    fn a_run_of_silence_is_still_fatal() {
        // The failure the abort was added for: a stale serial handle
        // answered every command with an error, and the capture wrote a
        // confident file with five settings in it.
        assert!(
            MAX_CONSECUTIVE_SILENCE <= 5,
            "a dead link must be caught quickly, not after the whole table"
        );
    }

    #[test]
    fn a_capture_error_says_what_it_asked_and_what_came_back() {
        let e = CaptureError {
            code: "FA".to_string(),
            answer: "ERR timed out".to_string(),
        };
        let text = e.to_string();
        assert!(text.contains("FA"), "must name the command: {text}");
        assert!(
            text.contains("ERR timed out"),
            "must quote the answer: {text}"
        );
        assert!(
            text.contains("nothing after it can be trusted"),
            "must say why it matters: {text}"
        );
    }

    #[test]
    fn volatile_readings_are_kept_out_of_snapshots() {
        // An S-meter moves with the band. Capturing it would make every
        // later verify report drift, and an alarm that always fires is one
        // nobody reads.
        for code in ["SM", "RM", "BY", "IF"] {
            assert!(VOLATILE.contains(&code), "{code} must be excluded");
        }
    }

    #[test]
    fn a_restore_never_powers_the_radio_off_or_keys_the_pa() {
        assert!(NEVER_RESTORE.contains(&"PS"), "PS0; powers the radio off");
        assert!(
            NEVER_RESTORE.contains(&"AC"),
            "AC starts an ATU tune, which keys the PA for up to 60 s"
        );
    }

    #[test]
    fn the_menu_read_is_never_captured_as_a_setting() {
        // `EX;` answers with whichever menu the panel is on. Captured as a
        // setting it would report drift every time the knob moved, and a
        // restore would write it back -- and `EX<nnn><vvvv>;` IS a menu
        // write, so restoring a captured "EX": "0000000" would quietly set
        // menu 0 to zero. Menus have their own section for this reason.
        assert!(VOLATILE.contains(&"EX"));
    }

    #[test]
    fn the_consequential_menus_include_the_two_that_look_like_a_broken_radio() {
        let numbers: Vec<u8> = CONSEQUENTIAL_MENUS.iter().map(|(n, _)| *n).collect();
        assert!(numbers.contains(&38), "TX inhibit");
        assert!(numbers.contains(&39), "linear amplifier relay");
        for (menu, label) in CONSEQUENTIAL_MENUS {
            assert!(*menu < MENU_COUNT);
            assert!(!label.is_empty());
        }
    }

    #[test]
    fn a_restore_that_wrote_menus_is_not_finished() {
        // Menu writes land silently. Until the operator has swept the
        // panel, a restore has changed the radio without establishing
        // anything, and must not report itself as done.
        let mut r = RestoreReport::default();
        assert!(!r.needs_verify_sweep(), "nothing written, nothing to check");
        r.menus_written.push(38);
        assert!(r.needs_verify_sweep());
    }

    #[test]
    fn a_restore_verified_by_a_sweep_is_a_drift_check_against_the_file() {
        // How the two halves join up: restore writes, the sweep produces a
        // fresh snapshot, and `drift` against the file is the confirmation.
        let mut file = snap();
        file.record_menu(38, 1, MenuSource::Panel);
        file.record_menu(39, 0, MenuSource::Panel);

        let mut swept = snap();
        swept.record_menu(38, 1, MenuSource::Panel);
        swept.record_menu(39, 1, MenuSource::Panel); // did not take

        let d = file.drift(&swept);
        assert_eq!(d.menus_changed.len(), 1);
        assert_eq!(d.menus_changed[0].what, "menu 039");
        assert!(!d.fully_verified());
    }

    #[test]
    fn a_turned_knob_is_not_drift() {
        // The first run of this against the physical radio warned
        // "recalibrate, every menu is suspect" because the AF gain had
        // moved from 019 to 035 -- which is what an AF gain does. An
        // alarm that fires when somebody adjusts the volume is one people
        // learn to mute.
        let mut stored = snap();
        stored.record_setting("AG", "019");
        stored.record_setting("MD", "2");
        let mut now = snap();
        now.record_setting("AG", "035");
        now.record_setting("MD", "2");

        let d = stored.drift(&now);
        assert!(!d.drifted(), "a turned knob must not read as drift");
        assert_eq!(d.operator_changed.len(), 1, "but it must still be reported");
        assert_eq!(d.operator_changed[0].what, "AG");
        assert!(d.settings_changed.is_empty());
    }

    #[test]
    fn a_changed_mode_is_still_drift_even_beside_a_turned_knob() {
        let mut stored = snap();
        stored.record_setting("AG", "019");
        stored.record_setting("MD", "2");
        let mut now = snap();
        now.record_setting("AG", "035");
        now.record_setting("MD", "3");

        let d = stored.drift(&now);
        assert!(d.drifted(), "a mode change is a reconfiguration");
        assert_eq!(d.settings_changed.len(), 1);
        assert_eq!(d.settings_changed[0].what, "MD");
        assert_eq!(d.operator_changed.len(), 1);
    }

    #[test]
    fn knobs_are_only_the_ones_turned_every_qso() {
        // Deliberately narrow. MD is excluded: a mode change is a
        // deliberate act about how the station is set up. SH/SL and IS are
        // front-panel knobs too, but are set for a band or a signal.
        assert_eq!(OPERATOR_CONTROLS, &["AG", "RG", "SQ", "FA", "FB"]);
        assert!(!OPERATOR_CONTROLS.contains(&"MD"), "a mode change is real");
        for k in OPERATOR_CONTROLS {
            assert!(!VOLATILE.contains(k), "{k} is a real setting, just a knob");
        }
    }

    #[test]
    fn tuning_the_dial_is_not_drift() {
        // Reported from the bench: "FA was 00014074055 now 00014074000 --
        // frequency should not be part of the settings". A 55 Hz move
        // raised "somebody has changed settings, every menu is suspect".
        let mut stored = snap();
        stored.record_setting("FA", "00014074055");
        stored.record_setting("MD", "2");
        let mut now = snap();
        now.record_setting("FA", "00014074000");
        now.record_setting("MD", "2");

        let d = stored.drift(&now);
        assert!(!d.drifted(), "tuning must not read as a reconfiguration");
        assert_eq!(d.operator_changed.len(), 1, "but it is still reported");
        assert_eq!(d.operator_changed[0].what, "FA");
    }

    #[test]
    fn the_dial_is_still_captured_and_restorable() {
        // Not counted as drift is not the same as not recorded: a restore
        // should put the radio back where it was.
        let mut s = snap();
        s.record_setting("FA", "00014074000");
        assert_eq!(
            s.settings.get("FA").map(String::as_str),
            Some("00014074000")
        );
        assert!(!VOLATILE.contains(&"FA"), "captured, unlike SM or EX");
        assert!(
            !NEVER_RESTORE.contains(&"FA"),
            "and written back on restore"
        );
    }

    #[test]
    fn a_status_where_only_knobs_moved_does_not_warn() {
        let status = CalibrationStatus::OnlyKnobsMoved {
            path: "b.json".into(),
            report: DriftReport {
                settings_changed: vec![],
                operator_changed: vec![Changed {
                    what: "AG".into(),
                    was: "019".into(),
                    now: "035".into(),
                }],
                menus_changed: vec![],
                menus_unchecked: vec![],
            },
        };
        assert!(!status.is_warning());
        let lines = status.lines().join("\n");
        assert!(lines.contains("matches"), "{lines}");
        assert!(
            lines.contains("AG was 019 now 035"),
            "still reported: {lines}"
        );
    }

    #[test]
    fn only_a_complete_matching_snapshot_is_not_a_warning() {
        let quiet = CalibrationStatus::Current {
            path: "b.json".into(),
            captured_at: "2026-09-07T00:00:00Z".into(),
        };
        assert!(!quiet.is_warning());

        for noisy in [
            CalibrationStatus::NotConfigured,
            CalibrationStatus::Unreadable {
                path: "b.json".into(),
                reason: "no such file".into(),
            },
            CalibrationStatus::Incomplete {
                path: "b.json".into(),
                captured: 2,
                missing: vec![1, 2],
            },
            CalibrationStatus::Drifted {
                path: "b.json".into(),
                report: DriftReport {
                    settings_changed: vec![],
                    operator_changed: vec![],
                    menus_changed: vec![],
                    menus_unchecked: vec![],
                },
            },
            CalibrationStatus::CouldNotCheck {
                path: "b.json".into(),
                reason: "timed out".into(),
            },
        ] {
            assert!(noisy.is_warning(), "{noisy:?} should warn");
        }
    }

    #[test]
    fn an_uncalibrated_server_is_told_how_to_fix_it() {
        // A warning that does not say what to do is noise somebody learns
        // to scroll past.
        let lines = CalibrationStatus::NotConfigured.lines().join("\n");
        assert!(lines.contains("unknown"));
        assert!(lines.contains("ts570d calibrate"), "must name the command");
        assert!(lines.contains("--calibration"), "and how to use the result");
    }

    #[test]
    fn an_incomplete_snapshot_names_the_menus_it_never_read() {
        let lines = CalibrationStatus::Incomplete {
            path: "b.json".into(),
            captured: 50,
            missing: vec![7, 38],
        }
        .lines()
        .join("\n");
        assert!(lines.contains("INCOMPLETE"));
        assert!(lines.contains("007"), "must name the gaps: {lines}");
        assert!(lines.contains("038"));
    }

    #[test]
    fn drift_warns_that_the_unreadable_menus_are_now_suspect_too() {
        // The point that is easy to miss and expensive to learn: a changed
        // CAT setting means somebody was at the panel, and the menus
        // nobody can read are the ones that will bite.
        let lines = CalibrationStatus::Drifted {
            path: "b.json".into(),
            report: DriftReport {
                settings_changed: vec![Changed {
                    what: "MD".into(),
                    was: "2".into(),
                    now: "3".into(),
                }],
                operator_changed: vec![],
                menus_changed: vec![],
                menus_unchecked: vec![],
            },
        }
        .lines()
        .join("\n");
        assert!(lines.contains("MD was 2 now 3"));
        assert!(
            lines.contains("every menu in that file is now suspect")
                || lines.contains("is now suspect"),
            "must escalate beyond the listed settings: {lines}"
        );
    }

    #[test]
    fn a_snapshot_missing_menus_is_incomplete_before_the_radio_is_asked() {
        // Ordering matters: a file that never covered every menu cannot be
        // made accurate by agreeing with the radio about the rest, so
        // there is no point spending reads to find that out.
        let mut s = snap();
        s.record_menu(0, 0, MenuSource::Panel);
        assert_eq!(s.missing_menus().len(), (MENU_COUNT - 1) as usize);
    }

    #[test]
    fn iso8601_formats_a_known_instant() {
        assert_eq!(iso8601_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601_utc(1_000_000_000), "2001-09-09T01:46:40Z");
        // A leap day, which is where a hand-rolled calendar goes wrong.
        assert_eq!(iso8601_utc(1_709_164_800), "2024-02-29T00:00:00Z");
    }
}
