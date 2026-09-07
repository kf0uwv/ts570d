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

//! `ts570d calibrate` — capture, verify and restore a radio's whole
//! configuration, menus included.
//!
//! The operator-facing half. What a TS-570D's configuration *is*, and why
//! CAT cannot read it alone, lives in [`radio::calibration`]; this drives
//! that against a real radio and talks to the person at the panel.
//!
//! # Why there is a person in the loop at all
//!
//! `EX;` answers only for the menu the front panel has selected. So the
//! fifty-two menus cannot be enumerated over CAT — somebody has to turn
//! the knob. The saving grace is that `EX;` names the menu it answers
//! for, so the instruction is one sentence, not fifty-two prompts:
//! *sweep the MENU knob from 00 to 51*. Any order works, backtracking
//! works, and stopping and resuming works.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cat_transport_core::{CatSession, TransportError};
use radio::calibration::{
    capture_aliased_menus, capture_settings, iso8601_utc, read_selected_menu, restore, DriftReport,
    Snapshot, CONSEQUENTIAL_MENUS, MENU_COUNT, RECALIBRATION_WARNING,
};
use radio::Ts570d;

/// Where a calibration snapshot lives unless told otherwise.
///
/// Beside `asound.state`, the sound card's mixer snapshot, because they
/// are the same kind of thing: per-station bench state that no amount of
/// querying the hardware can reconstruct.
///
/// Returns `None` only when there is no home directory to put it in, in
/// which case the server reports itself uncalibrated rather than guessing
/// at a path.
pub fn default_path() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .or_else(|| std::env::var_os("APPDATA"))?;
    Some(
        std::path::PathBuf::from(home)
            .join(".config")
            .join("ts570d")
            .join("calibration.json"),
    )
}

/// What the operator asked for.
pub enum Mode {
    Capture { out: String },
    Verify { file: String },
    Restore { file: String },
}

/// How long between `EX;` polls during a sweep.
///
/// Fast enough that a knob turned at a normal pace never skips a menu,
/// slow enough not to saturate a 9600-baud link that the operator may
/// also be watching in a log.
const POLL: Duration = Duration::from_millis(120);

/// Consecutive unintelligible polls that mean the link has gone.
///
/// Generous, because a miss mid-sweep is ordinary — the operator may be
/// between detents. At [`POLL`] this is about four seconds of silence.
const STALL_POLLS: u32 = 32;

pub async fn run<S>(radio: &mut Ts570d<S>, mode: Mode)
where
    S: CatSession<Error = TransportError>,
{
    match mode {
        Mode::Capture { out } => capture(radio, &out).await,
        Mode::Verify { file } => verify(radio, &file).await,
        Mode::Restore { file } => restore_from(radio, &file).await,
    }
}

// ---------------------------------------------------------------------------
// Capture
// ---------------------------------------------------------------------------

async fn capture<S>(radio: &mut Ts570d<S>, out: &str)
where
    S: CatSession<Error = TransportError>,
{
    let mut snapshot = Snapshot::new("TS-570D", now_iso());

    print!("Reading CAT-readable settings... ");
    let _ = std::io::stdout().flush();
    let n = match capture_settings(radio, &mut snapshot).await {
        Ok(n) => n,
        Err(e) => {
            // Nothing is written. A partial capture from a link that
            // stopped answering looks exactly like a real one, and the
            // whole point of the file is that it can be relied on later.
            println!();
            eprintln!("error: {e}");
            eprintln!("No file written. Fix the link and run this again.");
            return;
        }
    };
    println!("{n} captured");

    let aliased = capture_aliased_menus(radio, &mut snapshot).await;
    if aliased > 0 {
        for (menu, reading) in &snapshot.menus {
            println!(
                "  menu {menu:03} = {} (read over CAT, no sweep needed)",
                reading.value
            );
        }
    }

    sweep(radio, &mut snapshot).await;
    write_snapshot(&snapshot, out);
    report_completeness(&snapshot);
}

// ---------------------------------------------------------------------------
// Verify
// ---------------------------------------------------------------------------

async fn verify<S>(radio: &mut Ts570d<S>, file: &str)
where
    S: CatSession<Error = TransportError>,
{
    let stored = match load(file) {
        Some(s) => s,
        None => return,
    };
    println!(
        "Comparing against {file} (captured {})\n",
        stored.captured_at
    );

    let mut current = Snapshot::new("TS-570D", now_iso());
    if let Err(e) = capture_settings(radio, &mut current).await {
        eprintln!("error: {e}");
        eprintln!("Cannot compare against a link that is not answering.");
        return;
    }
    capture_aliased_menus(radio, &mut current).await;

    println!(
        "CAT-readable settings and {} aliased menu(s) re-read.",
        current.menus.len()
    );
    if ask("Sweep the MENU knob to check the other menus too?") {
        sweep(radio, &mut current).await;
    }

    print_drift(&stored.drift(&current));
}

// ---------------------------------------------------------------------------
// Restore
// ---------------------------------------------------------------------------

async fn restore_from<S>(radio: &mut Ts570d<S>, file: &str)
where
    S: CatSession<Error = TransportError>,
{
    let stored = match load(file) {
        Some(s) => s,
        None => return,
    };

    println!("Restoring from {file} (captured {})", stored.captured_at);
    let consequential: Vec<&(u8, &str)> = CONSEQUENTIAL_MENUS
        .iter()
        .filter(|(n, _)| stored.menus.contains_key(n))
        .collect();
    if !consequential.is_empty() {
        println!("\nThis will write menus that change what the radio does:");
        for (menu, label) in &consequential {
            let v = stored.menus[menu].value;
            println!("  menu {menu:03} = {v}   {label}");
        }
    }
    if !ask("\nProceed?") {
        println!("Nothing written.");
        return;
    }

    let report = restore(radio, &stored).await;
    println!(
        "\n{} setting(s) written and confirmed, {} menu(s) written blind.",
        report.settings_confirmed.len(),
        report.menus_written.len()
    );
    for failed in &report.settings_failed {
        println!(
            "  ! {} did not take: wanted {}, radio reports {}",
            failed.what, failed.was, failed.now
        );
    }

    if !report.needs_verify_sweep() {
        return;
    }

    // Not optional. A menu write lands silently, so until the panel has
    // been swept this has changed the radio and established nothing.
    println!(
        "\nMenu writes cannot be read back over CAT, so none of them is confirmed yet.\n\
         Sweep the MENU knob now to check what actually landed."
    );
    let mut swept = Snapshot::new("TS-570D", now_iso());
    // Aliased menus confirm themselves — no reason to make the operator
    // sweep to a menu a CAT command already reports.
    let confirmed_over_cat = capture_aliased_menus(radio, &mut swept).await;
    if confirmed_over_cat > 0 {
        println!("({confirmed_over_cat} menu(s) confirmed over CAT already.)");
    }
    sweep(radio, &mut swept).await;

    let drift = stored.drift(&swept);
    if drift.menus_changed.is_empty() && drift.menus_unchecked.is_empty() {
        println!("\nRestore CONFIRMED: every menu in the file reads back as written.");
    } else {
        println!("\nRestore NOT fully confirmed.");
        for c in &drift.menus_changed {
            println!("  ! {} is {} — the file says {}", c.what, c.now, c.was);
        }
        if !drift.menus_unchecked.is_empty() {
            println!(
                "  {} menu(s) never swept, so nothing is known about them: {}",
                drift.menus_unchecked.len(),
                join(&drift.menus_unchecked)
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The sweep
// ---------------------------------------------------------------------------

/// Watch `EX;` while the operator turns the knob.
///
/// Records whatever menu the radio names, so the operator is never asked
/// to confirm they selected the right one — the radio says which it is.
async fn sweep<S>(radio: &mut Ts570d<S>, snapshot: &mut Snapshot)
where
    S: CatSession<Error = TransportError>,
{
    println!(
        "\nTurn the MENU knob slowly from 00 through {:02}.\n\
         Any order is fine. Press Enter when you are done.\n",
        MENU_COUNT - 1
    );

    let stop = stop_on_enter();
    let mut last_shown = usize::MAX;
    let mut silent = 0u32;

    while !stop.load(Ordering::Relaxed) {
        match read_selected_menu(radio).await {
            Ok(Some((menu, value))) => {
                silent = 0;
                let fresh = snapshot.menus.get(&menu).map(|r| r.value) != Some(value);
                snapshot.record_menu(menu, value, radio::calibration::MenuSource::Panel);
                if fresh {
                    println!("  menu {menu:03} = {value}");
                }
            }
            // A single miss is ordinary mid-sweep; a run of them means the
            // link has gone, and a sweep that spins silently forever would
            // leave the operator turning a knob at nothing.
            _ => {
                silent += 1;
                if silent >= STALL_POLLS {
                    println!(
                        "\n\nThe radio stopped answering ({} polls). Sweep abandoned; \
                         what was captured so far is kept.",
                        STALL_POLLS
                    );
                    return;
                }
            }
        }
        let have = snapshot.menus.len();
        if have != last_shown {
            last_shown = have;
            print!("\r  {have}/{MENU_COUNT} captured");
            let _ = std::io::stdout().flush();
        }
        if snapshot.is_complete() {
            println!("\n\nAll {MENU_COUNT} menus captured.");
            return;
        }
        monoio::time::sleep(POLL).await;
    }
    println!();
}

/// A flag the operator sets by pressing Enter.
///
/// A thread rather than async stdin: it is one blocking read for the life
/// of the sweep, and this way the sweep loop stays a plain poll on both
/// Linux and Windows.
fn stop_on_enter() -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&flag);
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        signal.store(true, Ordering::Relaxed);
    });
    flag
}

// ---------------------------------------------------------------------------
// Reporting and files
// ---------------------------------------------------------------------------

fn report_completeness(snapshot: &Snapshot) {
    let missing = snapshot.missing_menus();
    if missing.is_empty() {
        println!(
            "\nComplete: all {MENU_COUNT} menus and {} settings.",
            snapshot.settings.len()
        );
    } else {
        println!(
            "\nINCOMPLETE: {} menu(s) were never read and are absent from the file: {}",
            missing.len(),
            join(&missing)
        );
    }
    println!("\n{RECALIBRATION_WARNING}");
}

fn print_drift(drift: &DriftReport) {
    println!();
    if drift.settings_changed.is_empty() && drift.menus_changed.is_empty() {
        println!("Nothing that could be checked has changed.");
    } else {
        for c in &drift.settings_changed {
            println!("  changed: {} was {} now {}", c.what, c.was, c.now);
        }
        for c in &drift.menus_changed {
            println!("  changed: {} was {} now {}", c.what, c.was, c.now);
        }
    }

    if !drift.menus_unchecked.is_empty() {
        // Said plainly, because "no drift detected" about a menu nobody
        // read would be a claim this cannot support.
        println!(
            "\n{} menu(s) were NOT checked — nothing is known about them either way: {}",
            drift.menus_unchecked.len(),
            join(&drift.menus_unchecked)
        );
    }

    if drift.fully_verified() {
        println!("\nVERIFIED: the radio matches the file.");
    } else if drift.drifted() {
        println!("\nRECALIBRATION ADVISED.");
        println!(
            "Something has been changed at the front panel, which makes every\n\
             menu this cannot read suspect — not only the ones shown above."
        );
    } else {
        // Declining the sweep is not evidence of a problem. Saying
        // "recalibration advised" here would conflate "nobody looked" with
        // "something is wrong", and an alarm that fires when nothing has
        // happened is one people learn to ignore.
        println!("\nNOT FULLY VERIFIED, but nothing is known to have changed.");
        println!("Sweep the MENU knob to confirm the menus above.");
    }
}

fn write_snapshot(snapshot: &Snapshot, out: &str) {
    match serde_json::to_string_pretty(snapshot) {
        Ok(json) => match std::fs::write(out, json) {
            Ok(()) => println!("\nWritten to {out}"),
            Err(e) => eprintln!("error: could not write {out}: {e}"),
        },
        Err(e) => eprintln!("error: could not serialise the snapshot: {e}"),
    }
}

fn load(file: &str) -> Option<Snapshot> {
    let text = std::fs::read_to_string(file)
        .map_err(|e| eprintln!("error: could not read {file}: {e}"))
        .ok()?;
    serde_json::from_str(&text)
        .map_err(|e| eprintln!("error: {file} is not a calibration snapshot: {e}"))
        .ok()
}

fn ask(question: &str) -> bool {
    print!("{question} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim(), "y" | "Y" | "yes")
}

fn join(menus: &[u8]) -> String {
    menus
        .iter()
        .map(|n| format!("{n:03}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn now_iso() -> String {
    iso8601_utc(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    )
}
