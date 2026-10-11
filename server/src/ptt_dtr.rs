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

//! Keying the transmitter on the **DTR line**, not with CAT `TX;`.
//!
//! On a station wired the way the ACC2-IF interface is, PTT is DTR through an
//! opto onto ACC2 pin 9 (PKS). That is not interchangeable with CAT `TX;`:
//! **pin 9 mutes the mic while keyed and `TX;` does not**, so a digital-mode
//! client keying over rigctl gets different audio behaviour depending on which
//! path is used. This module is the DTR path.
//!
//! # Why the two directions are not symmetric
//!
//! **Assert goes through the broker's ordered queue.** Keying must not
//! overtake the command that set the mode or frequency, and the queue is the
//! only thing that orders it against CAT traffic on the same wire.
//!
//! **Release does not wait for the queue** (radio-cat-rs ADR 0022). Every
//! de-keying path here -- a client's `T 0`, a dropped connection, the
//! hard key-down watchdog -- uses `BrokerCatSession::release_now_bypassing_queue`,
//! which drops DTR at once on the broker's release lines: an owned duplicate
//! of the port's modem-control lines, reachable without the worker's
//! exclusive borrow of the session. A release routed through the queue
//! could sit behind a CAT exchange that hits the 2 s read timeout or the
//! 5 s broker timeout, *while the radio is transmitting*. A line change is
//! safe to take out of order because it never touches the byte stream: it
//! cannot corrupt a frame in flight. Ordering only ever mattered for the
//! assert.
//!
//! If a transport offers no release lines the broker falls back to the
//! ordered queue (and warns once); the release still happens, just not
//! promptly. Every serial port this station keys on has them.
//!
//! # Linux and Windows
//!
//! Releases are the same on both: they run on the caller's thread, so the
//! disconnect release completes inside `Drop` without needing any runtime
//! to poll a spawned task. The hard key-down watchdog is a `monoio` task on
//! Linux and a plain OS thread on Windows (where there is no `monoio`; see
//! `docs/adr/0006-windows-concurrency-model.md`), woken early by a
//! cancellation channel. Both obey the same rule,
//! [`PttDtr::watchdog_may_release`].
//!
//! # RTS is not an option on this radio
//!
//! The TS-570D uses RTS as receive-enable and withholds CAT responses while it
//! is low. A "PTT via RTS" configuration would therefore silence CAT, and
//! would present as responses failing to arrive — indistinguishable from a
//! desynchronised stream. Only DTR is offered here.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::task::{Context, Wake, Waker};
use std::time::{Duration, Instant};
use tracing::{info, warn};

use cat_server::{BrokerCatSession, TaskFn};

/// How long a key-down may last before it is released regardless of what the
/// client does.
///
/// A rigctl client can send `T 1` and then have its TCP connection drop.
/// Nothing in the rigctl protocol requires a station to survive that; this
/// station does. Belt to the disconnect braces in [`PttDtr::release`].
pub const MAX_KEY_DOWN: Duration = Duration::from_secs(120);

/// Keys and releases PTT on the DTR line.
pub struct PttDtr {
    /// Shared with the watchdog, which on Windows runs on its own thread --
    /// hence atomics rather than `Rc<Cell<_>>`.
    keyed: Arc<AtomicBool>,
    /// Which key-down we are on.
    ///
    /// Incremented by every [`PttDtr::key`]. A watchdog captures the value
    /// current when it was armed and refuses to act once it has moved, so
    /// it can only ever end the transmission it was armed for.
    ///
    /// Without this the watchdog tested `keyed` alone, which cannot tell
    /// "still keyed from the transmission I am guarding" from "keyed again
    /// since". Nothing cancelled a watchdog on release, so every key-down
    /// left one running for the full `MAX_KEY_DOWN`, and any transmission
    /// that happened to start two minutes after an earlier one was cut
    /// down by the earlier one's timer.
    ///
    /// Observed on the bench 2026-09-09 with WSJT-X on FT8: keyed at
    /// 06:11:00.088795, force-released at 06:11:00.392249 -- 304 ms into a
    /// 13.8-second transmission -- exactly 120.0016 s after the key-down
    /// at 06:09:00.390667. The operator hears the transmission chop.
    generation: Arc<AtomicU64>,
    /// The hard key-down timeout this instance arms. [`MAX_KEY_DOWN`]
    /// outside tests.
    max_key_down: Duration,
    /// Wakes the armed watchdog thread so it re-checks whether its
    /// key-down is over, instead of sleeping out `max_key_down` and holding
    /// a broker handle that long. Dropping it (with this `PttDtr`) wakes it
    /// too.
    #[cfg(not(target_os = "linux"))]
    watchdog_wake: std::sync::Mutex<Option<mpsc::Sender<()>>>,
}

impl Default for PttDtr {
    fn default() -> Self {
        Self::new()
    }
}

impl PttDtr {
    pub fn new() -> Self {
        Self {
            keyed: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
            max_key_down: MAX_KEY_DOWN,
            #[cfg(not(target_os = "linux"))]
            watchdog_wake: std::sync::Mutex::new(None),
        }
    }

    /// A `PttDtr` whose watchdog fires after `max_key_down` -- so a test
    /// can watch it fire without waiting two minutes.
    #[cfg(test)]
    fn with_max_key_down(max_key_down: Duration) -> Self {
        Self {
            max_key_down,
            ..Self::new()
        }
    }

    /// Whether this station believes PTT is currently asserted.
    pub fn is_keyed(&self) -> bool {
        self.keyed.load(Ordering::SeqCst)
    }

    /// Build the closure that drives DTR one way or the other.
    fn line_task(assert: bool) -> TaskFn {
        Box::new(move |lines| match lines {
            Some(l) => l
                .set_dtr(assert)
                .map(|()| {
                    if assert {
                        b"PTT-ON".to_vec()
                    } else {
                        b"PTT-OFF".to_vec()
                    }
                })
                .map_err(|e| {
                    format!(
                        "could not {} DTR: {e}",
                        if assert { "assert" } else { "release" }
                    )
                }),
            None => Err("this transport has no modem lines; PTT via DTR is \
                         not available on it"
                .to_string()),
        })
    }

    /// Whether a watchdog armed at `armed_generation` may end the current
    /// key-down.
    ///
    /// Two conditions, and the second is the one that was missing. The
    /// radio must still be keyed, *and* it must still be keyed for the
    /// same transmission this watchdog was armed for. A watchdog that
    /// outlives its own key-down is guarding something that has already
    /// ended, and the next transmission is not its to end.
    ///
    /// A pure function so the rule can be tested in microseconds. The
    /// alternative is a test that sleeps for `MAX_KEY_DOWN`, which is two
    /// minutes per case and would never have been written.
    fn watchdog_may_release(keyed: bool, current_generation: u64, armed_generation: u64) -> bool {
        keyed && current_generation == armed_generation
    }

    /// Release DTR now, without the caller awaiting anything -- the
    /// failsafe path, shared by the disconnect release and the watchdog.
    ///
    /// Takes an owned session so it can run from `Drop` or a watchdog,
    /// neither of which can borrow the caller's.
    ///
    /// The bypassing release completes on its first poll whenever the
    /// broker has release lines, so it is polled once here, synchronously:
    /// by the time this returns, DTR is down. Only if the transport offered
    /// no release lines -- the broker then falls back to its queue and the
    /// first poll is `Pending` -- is the rest handed to [`detach`], which
    /// still finishes it, just not promptly.
    fn release_detached(keyed: Arc<AtomicBool>, session: BrokerCatSession, why: &'static str) {
        if !keyed.load(Ordering::SeqCst) {
            return;
        }
        warn!("PTT: released ({why}) -- this was NOT a client asking to stop");
        let mut release = Box::pin(async move {
            let out = session
                .release_now_bypassing_queue(Self::line_task(false))
                .await;
            match out {
                Some(out) if out.starts_with(b"ERR ") => {
                    warn!(
                        "PTT: release failed: {}",
                        String::from_utf8_lossy(&out[4..])
                    );
                }
                Some(_) => {}
                None => warn!("PTT: release failed: broker worker has shut down"),
            }
            keyed.store(false, Ordering::SeqCst);
        });
        let waker = Waker::from(Arc::new(NoopWake));
        if release
            .as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
        {
            detach(release);
        }
    }

    /// Release PTT because the client that keyed it has gone away.
    ///
    /// A rigctl client can send `T 1` and then have its TCP connection drop.
    /// Nothing in the rigctl protocol requires a station to survive that; a
    /// station with a transmitter on the end of it must.
    ///
    /// Synchronous: DTR is down when this returns (see
    /// [`Self::release_detached`]), so it is safe to call from `Drop` on
    /// either platform.
    pub fn release_on_disconnect(&self, session: BrokerCatSession) {
        Self::release_detached(
            Arc::clone(&self.keyed),
            session,
            "the client that keyed it disconnected",
        );
        self.wake_watchdog();
    }

    /// Assert DTR, ordered against CAT traffic.
    pub async fn key(&self, session: &BrokerCatSession) -> Result<(), String> {
        let out = session
            .submit_task(Self::line_task(true))
            .await
            .ok_or_else(|| "broker worker has shut down".to_string())?;
        if out.starts_with(b"ERR ") {
            return Err(String::from_utf8_lossy(&out[4..]).into_owned());
        }
        self.keyed.store(true, Ordering::SeqCst);
        // A new key-down, so any watchdog still running from an earlier one
        // is now guarding a transmission that has ended and must stand
        // down.
        self.generation.fetch_add(1, Ordering::SeqCst);
        // Every key and every release is logged, with what caused it.
        // Nothing here said anything before, so "the radio flips back to
        // receive part-way through a transmission" could only be guessed
        // at -- a client's `T 0`, a dropped connection and the watchdog
        // all look identical from outside.
        info!("PTT: keyed (DTR asserted)");

        // Arm the hard timeout. Independent of the client: it fires whether
        // or not anything ever sends `T 0`, and whether or not the client is
        // still connected. This is the last line of defence against a
        // transmitter left keyed.
        self.arm_watchdog(session)
    }

    /// The Linux watchdog: a `monoio` task that sleeps out the key-down.
    #[cfg(target_os = "linux")]
    fn arm_watchdog(&self, session: &BrokerCatSession) -> Result<(), String> {
        let keyed = Arc::clone(&self.keyed);
        let generation = Arc::clone(&self.generation);
        // The key-down this watchdog is responsible for. It may only end
        // this one: a later transmission is somebody else's to guard, and
        // a watchdog that outlives its own key-down must do nothing.
        let mine = generation.load(Ordering::SeqCst);
        let max_key_down = self.max_key_down;
        let watchdog_session = BrokerCatSession::new(session.handle(), session.client_id());
        monoio::spawn(async move {
            monoio::time::sleep(max_key_down).await;
            if Self::watchdog_may_release(
                keyed.load(Ordering::SeqCst),
                generation.load(Ordering::SeqCst),
                mine,
            ) {
                Self::release_detached(
                    keyed,
                    watchdog_session,
                    "the hard key-down timeout expired",
                );
            }
        });
        Ok(())
    }

    /// The Windows watchdog: an OS thread (there is no `monoio` here), woken
    /// early through `watchdog_wake` -- see [`watch_key_down`].
    #[cfg(not(target_os = "linux"))]
    fn arm_watchdog(&self, session: &BrokerCatSession) -> Result<(), String> {
        let keyed = Arc::clone(&self.keyed);
        let generation = Arc::clone(&self.generation);
        let mine = generation.load(Ordering::SeqCst);
        let max_key_down = self.max_key_down;
        let (handle, client_id) = (session.handle(), session.client_id());
        let (wake, cancel) = mpsc::channel();
        // Replacing the previous sender wakes the previous watchdog, which
        // then sees the generation has moved and stands down.
        self.replace_watchdog_wake(Some(wake));
        let spawned = std::thread::Builder::new()
            .name("ptt-watchdog".to_string())
            .spawn(move || {
                let release_keyed = Arc::clone(&keyed);
                watch_key_down(
                    &cancel,
                    max_key_down,
                    &keyed,
                    &generation,
                    mine,
                    move || {
                        Self::release_detached(
                            release_keyed,
                            BrokerCatSession::new(handle, client_id),
                            "the hard key-down timeout expired",
                        );
                    },
                );
            });
        if let Err(e) = spawned {
            // No watchdog means no transmission: drop the key we just
            // asserted rather than leave it unguarded.
            Self::release_detached(
                Arc::clone(&self.keyed),
                BrokerCatSession::new(session.handle(), session.client_id()),
                "the key-down watchdog could not be started",
            );
            return Err(format!("could not start the PTT watchdog: {e}"));
        }
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    fn replace_watchdog_wake(&self, wake: Option<mpsc::Sender<()>>) {
        let mut slot = self
            .watchdog_wake
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(previous) = std::mem::replace(&mut *slot, wake) {
            let _ = previous.send(());
        }
    }

    /// Wake an armed watchdog so it notices its key-down has ended. The
    /// Linux watchdog is not woken; it re-checks when its sleep ends.
    fn wake_watchdog(&self) {
        #[cfg(not(target_os = "linux"))]
        self.replace_watchdog_wake(None);
    }

    /// De-assert DTR now, skipping the broker's queue (radio-cat-rs ADR
    /// 0022) -- see this module's doc comment.
    pub async fn release(&self, session: &BrokerCatSession) -> Result<(), String> {
        info!("PTT: released (a client asked to stop transmitting)");
        let result = session
            .release_now_bypassing_queue(Self::line_task(false))
            .await;
        // Clear the flag even if the release reported failure: believing we
        // are still keyed when we may not be is the safer error, and the
        // caller is told.
        self.keyed.store(false, Ordering::SeqCst);
        self.wake_watchdog();
        match result {
            Some(out) if out.starts_with(b"ERR ") => {
                Err(String::from_utf8_lossy(&out[4..]).into_owned())
            }
            Some(_) => Ok(()),
            None => Err("broker worker has shut down".to_string()),
        }
    }
}

/// Wait out one key-down's hard timeout on an OS thread, and call `release`
/// if that same key-down is still live at the deadline. Returns whether it
/// released.
///
/// `cancel` only wakes the thread early to re-check
/// [`PttDtr::watchdog_may_release`]; it never by itself stands the watchdog
/// down. A watchdog exits early only when the transmission it guards has
/// ended, so a wake from a dropped `PttDtr` whose disconnect release did not
/// complete still leaves it to fire at the deadline.
///
/// Compiled everywhere so it is tested on Linux too; only Windows arms it.
#[cfg_attr(target_os = "linux", allow(dead_code))]
fn watch_key_down(
    cancel: &mpsc::Receiver<()>,
    max_key_down: Duration,
    keyed: &AtomicBool,
    generation: &AtomicU64,
    mine: u64,
    release: impl FnOnce(),
) -> bool {
    let deadline = Instant::now() + max_key_down;
    let mut orphaned = false;
    loop {
        if !PttDtr::watchdog_may_release(
            keyed.load(Ordering::SeqCst),
            generation.load(Ordering::SeqCst),
            mine,
        ) {
            return false;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            release();
            return true;
        }
        if orphaned {
            std::thread::sleep(left);
        } else if let Err(mpsc::RecvTimeoutError::Disconnected) = cancel.recv_timeout(left) {
            orphaned = true;
        }
    }
}

/// A waker for the one synchronous poll in [`PttDtr::release_detached`].
/// If that poll is `Pending`, the future is re-polled by its new owner with
/// a real waker.
struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

/// Finish a release that could not complete synchronously (queue fallback).
#[cfg(target_os = "linux")]
fn detach<F: Future<Output = ()> + 'static>(release: Pin<Box<F>>) {
    monoio::spawn(release);
}

/// Finish a release that could not complete synchronously (queue fallback).
#[cfg(not(target_os = "linux"))]
fn detach<F: Future<Output = ()> + Send + 'static>(release: Pin<Box<F>>) {
    std::thread::spawn(move || cat_server::block_on::block_on(release));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_ptt_is_not_keyed() {
        assert!(!PttDtr::new().is_keyed());
    }

    #[test]
    fn release_on_disconnect_is_a_no_op_when_not_keyed() {
        // Must not spawn work (or touch a line) for a connection that never
        // transmitted — the overwhelmingly common case.
        let ptt = PttDtr::new();
        assert!(!ptt.is_keyed());
        // No runtime is running here; if this tried to spawn, it would panic.
        // That it returns quietly is the assertion.
        let (handle, _log) = {
            let (session, log) = StallingSession::new(true);
            let (_worker, handle) =
                cat_server::build_with_timeout(session, &radio::TS570D_COMMAND_TABLE, STALL);
            (handle, log)
        };
        ptt.release_on_disconnect(session(&handle));
        assert!(!ptt.is_keyed());
    }

    #[test]
    fn a_stale_watchdog_does_not_end_a_later_transmission() {
        // The chuttering bug, as a rule rather than a story.
        //
        // Every key-down armed a watchdog and nothing cancelled it on
        // release, so each one ran the full `MAX_KEY_DOWN` and then tested
        // `keyed` alone -- which cannot tell "still keyed from the
        // transmission I am guarding" from "keyed again since". Any
        // transmission starting roughly two minutes after an earlier one
        // was cut down by the earlier one's timer.
        //
        // Observed on the bench 2026-09-09, WSJT-X on FT8: keyed at
        // 06:11:00.088795, force-released at 06:11:00.392249 -- 304 ms
        // into a 13.8 s transmission -- which is 120.0016 s after the
        // key-down at 06:09:00.390667. Within 1.6 ms of `MAX_KEY_DOWN`
        // measured from the *wrong* key-down.
        assert!(
            !PttDtr::watchdog_may_release(true, 2, 1),
            "a watchdog armed for key-down 1 must not end key-down 2"
        );
    }

    #[test]
    fn a_watchdog_still_ends_the_transmission_it_was_armed_for() {
        // The other half. This is a safety failsafe against a transmitter
        // left keyed by a client that vanished, and narrowing it must not
        // disarm it.
        assert!(
            PttDtr::watchdog_may_release(true, 7, 7),
            "a watchdog must still end a key-down that is genuinely overrunning"
        );
    }

    #[test]
    fn a_watchdog_does_nothing_once_the_radio_is_unkeyed() {
        assert!(!PttDtr::watchdog_may_release(false, 3, 3));
        assert!(!PttDtr::watchdog_may_release(false, 4, 3));
    }

    #[test]
    fn every_key_down_gets_its_own_generation() {
        // What makes the rule above able to tell them apart. If two
        // key-downs ever shared a generation, the stale watchdog would be
        // indistinguishable from the live one again.
        let ptt = PttDtr::new();
        let first = ptt.generation.load(Ordering::SeqCst);
        ptt.generation.fetch_add(1, Ordering::SeqCst);
        let second = ptt.generation.load(Ordering::SeqCst);
        ptt.generation.fetch_add(1, Ordering::SeqCst);
        let third = ptt.generation.load(Ordering::SeqCst);
        assert_ne!(first, second);
        assert_ne!(second, third);
        assert_ne!(first, third);
    }

    #[test]
    fn the_hard_timeout_is_bounded_and_not_absurd() {
        // The point of this constant is that it exists. A client that drops
        // its connection mid-transmission must not leave the radio keyed
        // forever, and 120 s is long enough for any legitimate over.
        assert!(MAX_KEY_DOWN.as_secs() > 0);
        assert!(MAX_KEY_DOWN.as_secs() <= 300);
    }

    // -----------------------------------------------------------------
    // Against a broker whose CAT exchange is stuck (radio-cat-rs ADR 0022)
    // -----------------------------------------------------------------
    //
    // An in-crate session double (rule 6: never a real transport). Its
    // `FA;` exchange never answers, like a radio that stopped talking
    // mid-read, so the broker's worker holds the session until its request
    // timeout. Every line change is logged with which handle made it:
    // `primary` is the session's own borrowed lines (reachable only through
    // the worker's queue), `release` the owned duplicate the broker keeps
    // for de-keying (ADR 0022). Tests run on Linux under a monoio runtime
    // and on Windows over the thread-based broker -- same assertions.

    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use async_trait::async_trait;
    use cat_framework::ResponseDisposition;
    use cat_server::ClientId;
    use cat_transport_core::{CatSession, ModemControlLines, TransportError};

    /// How long the broker lets the stuck exchange run before giving up.
    const STALL: Duration = Duration::from_millis(1500);
    /// What "did not wait for the queue" means.
    const PROMPT: Duration = Duration::from_millis(100);

    type EventLog = Arc<Mutex<Vec<String>>>;

    fn logged(log: &EventLog) -> Vec<String> {
        log.lock().unwrap().clone()
    }

    fn position(events: &[String], entry: &str) -> Option<usize> {
        events.iter().position(|e| e == entry)
    }

    struct RecordingLines {
        tag: &'static str,
        log: EventLog,
    }

    impl ModemControlLines for RecordingLines {
        fn set_rts(&self, asserted: bool) -> Result<(), TransportError> {
            let entry = format!("{} rts={asserted}", self.tag);
            self.log.lock().unwrap().push(entry);
            Ok(())
        }
        fn set_dtr(&self, asserted: bool) -> Result<(), TransportError> {
            let entry = format!("{} dtr={asserted}", self.tag);
            self.log.lock().unwrap().push(entry);
            Ok(())
        }
        fn read_cts(&self) -> Result<bool, TransportError> {
            Ok(false)
        }
        fn read_dsr(&self) -> Result<bool, TransportError> {
            Ok(false)
        }
        fn read_dcd(&self) -> Result<bool, TransportError> {
            Ok(false)
        }
    }

    struct StallingSession {
        log: EventLog,
        primary: RecordingLines,
        with_release_lines: bool,
    }

    impl StallingSession {
        fn new(with_release_lines: bool) -> (Self, EventLog) {
            let log = EventLog::default();
            let session = Self {
                with_release_lines,
                primary: RecordingLines {
                    tag: "primary",
                    log: Arc::clone(&log),
                },
                log: Arc::clone(&log),
            };
            (session, log)
        }
    }

    /// Logs `cat over` however the exchange ends -- answered or cancelled
    /// by the broker's timeout.
    struct ExchangeOver(EventLog, String);

    impl Drop for ExchangeOver {
        fn drop(&mut self) {
            let entry = format!("cat over {}", self.1);
            self.0.lock().unwrap().push(entry);
        }
    }

    #[async_trait(?Send)]
    impl CatSession for StallingSession {
        type Error = TransportError;

        async fn execute(
            &mut self,
            request: &[u8],
            response: &mut Vec<u8>,
        ) -> Result<ResponseDisposition, Self::Error> {
            let req = String::from_utf8_lossy(request).into_owned();
            self.log.lock().unwrap().push(format!("cat begin {req}"));
            let _over = ExchangeOver(Arc::clone(&self.log), req);
            if request == b"FA;" {
                std::future::pending::<()>().await;
            }
            response.extend_from_slice(b"ID018;");
            Ok(ResponseDisposition::ResponseWritten)
        }

        fn modem_lines(&self) -> Option<&dyn ModemControlLines> {
            Some(&self.primary)
        }

        fn release_lines(&self) -> Option<Box<dyn ModemControlLines + Send + Sync>> {
            self.with_release_lines.then(|| {
                Box::new(RecordingLines {
                    tag: "release",
                    log: Arc::clone(&self.log),
                }) as Box<dyn ModemControlLines + Send + Sync>
            })
        }
    }

    /// Build a broker over a fresh [`StallingSession`] and start its worker.
    /// Must be called inside [`run`].
    fn broker() -> (cat_server::BrokerHandle, EventLog) {
        broker_with(true)
    }

    fn broker_with(release_lines: bool) -> (cat_server::BrokerHandle, EventLog) {
        let (session, log) = StallingSession::new(release_lines);
        let (worker, handle) =
            cat_server::build_with_timeout(session, &radio::TS570D_COMMAND_TABLE, STALL);
        #[cfg(target_os = "linux")]
        monoio::spawn(worker.run());
        #[cfg(not(target_os = "linux"))]
        std::thread::spawn(move || worker.run());
        (handle, log)
    }

    fn session(handle: &cat_server::BrokerHandle) -> BrokerCatSession {
        BrokerCatSession::new(handle.clone(), ClientId::from_raw(1))
    }

    /// Drive a test body to completion on this platform's executor.
    fn run<F: std::future::Future<Output = ()>>(body: F) {
        #[cfg(target_os = "linux")]
        monoio::RuntimeBuilder::<monoio::LegacyDriver>::new()
            .enable_timer()
            .build()
            .expect("monoio runtime")
            .block_on(body);
        #[cfg(not(target_os = "linux"))]
        cat_server::block_on::block_on(body);
    }

    /// Run `work` concurrently with the test body.
    #[cfg(target_os = "linux")]
    fn in_background<F: std::future::Future<Output = ()> + 'static>(work: F) {
        monoio::spawn(work);
    }

    #[cfg(not(target_os = "linux"))]
    fn in_background<F: std::future::Future<Output = ()> + Send + 'static>(work: F) {
        std::thread::spawn(move || cat_server::block_on::block_on(work));
    }

    async fn pause(d: Duration) {
        #[cfg(target_os = "linux")]
        monoio::time::sleep(d).await;
        #[cfg(not(target_os = "linux"))]
        std::thread::sleep(d);
    }

    /// Submit the `FA;` that hangs, and return once the worker is inside it.
    async fn stall_the_wire(handle: &cat_server::BrokerHandle, log: &EventLog) {
        let h = handle.clone();
        in_background(async move {
            let _ = h.submit(ClientId::from_raw(9), b"FA;".to_vec()).await;
        });
        until_logged(log, "cat begin FA;").await;
    }

    async fn until_logged(log: &EventLog, entry: &str) {
        let give_up = Instant::now() + STALL * 2;
        while position(&logged(log), entry).is_none() {
            assert!(
                Instant::now() < give_up,
                "never logged {entry:?}: {:?}",
                logged(log)
            );
            pause(Duration::from_millis(1)).await;
        }
    }

    #[test]
    fn release_does_not_wait_behind_a_stalled_cat_exchange() {
        run(async {
            let (handle, log) = broker();
            let cat = session(&handle);
            let ptt = PttDtr::new();
            stall_the_wire(&handle, &log).await;

            let started = Instant::now();
            let out = ptt.release(&cat).await;
            let took = started.elapsed();

            assert_eq!(out, Ok(()));
            assert!(took < PROMPT, "a client's `T 0` took {took:?}");
            let events = logged(&log);
            assert!(
                position(&events, "release dtr=false").is_some(),
                "released on the broker's release lines: {events:?}"
            );
            assert!(
                position(&events, "cat over FA;").is_none(),
                "released during the stuck exchange, not after it: {events:?}"
            );
        });
    }

    #[test]
    fn watchdog_release_does_not_wait_behind_a_stalled_cat_exchange() {
        let max = Duration::from_millis(200);
        run(async move {
            let (handle, log) = broker();
            let cat = session(&handle);
            let ptt = PttDtr::with_max_key_down(max);
            let keyed_at = Instant::now();
            ptt.key(&cat).await.expect("key");
            stall_the_wire(&handle, &log).await;

            until_logged(&log, "release dtr=false").await;
            let took = keyed_at.elapsed();

            assert!(took < max + PROMPT, "watchdog released after {took:?}");
            assert!(!ptt.is_keyed());
            let events = logged(&log);
            assert!(
                position(&events, "cat over FA;").is_none(),
                "released during the stuck exchange, not after it: {events:?}"
            );
        });
    }

    #[test]
    fn disconnect_release_completes_during_a_stalled_cat_exchange() {
        run(async {
            let (handle, log) = broker();
            let cat = session(&handle);
            let ptt = PttDtr::new();
            ptt.key(&cat).await.expect("key");
            stall_the_wire(&handle, &log).await;

            let started = Instant::now();
            ptt.release_on_disconnect(session(&handle));
            // No await between the call and these checks: the release has
            // to have happened by the time `Drop` returns, not whenever a
            // spawned task next gets polled.
            let took = started.elapsed();

            assert!(took < PROMPT, "disconnect release took {took:?}");
            assert!(!ptt.is_keyed());
            let events = logged(&log);
            assert!(
                position(&events, "release dtr=false").is_some(),
                "released before release_on_disconnect returned: {events:?}"
            );
            assert!(position(&events, "cat over FA;").is_none(), "{events:?}");
        });
    }

    #[test]
    fn key_still_waits_behind_a_queued_cat_command() {
        // The other direction stays ordered: keying must not overtake the
        // command that set the mode or frequency.
        run(async {
            let (handle, log) = broker();
            let cat = session(&handle);
            stall_the_wire(&handle, &log).await;

            in_background(async move {
                let ptt = PttDtr::new();
                ptt.key(&cat).await.expect("key");
            });
            pause(PROMPT).await;
            let events = logged(&log);
            assert!(
                !events.iter().any(|e| e.ends_with("dtr=true")),
                "keyed while the earlier exchange was still in flight: {events:?}"
            );

            until_logged(&log, "primary dtr=true").await;
            let events = logged(&log);
            assert!(
                position(&events, "cat over FA;") < position(&events, "primary dtr=true"),
                "keyed only after the earlier exchange, on the session's own lines: {events:?}"
            );
        });
    }

    #[test]
    fn without_release_lines_a_disconnect_release_still_happens_via_the_queue() {
        // The fallback: a transport with no release lines cannot release
        // synchronously, so the rest is detached -- and must still run.
        run(async {
            let (handle, log) = broker_with(false);
            let cat = session(&handle);
            let ptt = PttDtr::new();
            ptt.key(&cat).await.expect("key");

            ptt.release_on_disconnect(session(&handle));

            until_logged(&log, "primary dtr=false").await;
            let give_up = Instant::now() + STALL;
            while ptt.is_keyed() {
                assert!(Instant::now() < give_up, "never cleared `keyed`");
                pause(Duration::from_millis(1)).await;
            }
        });
    }

    // -----------------------------------------------------------------
    // The OS-thread watchdog loop (armed on Windows; tested everywhere)
    // -----------------------------------------------------------------

    /// Run [`watch_key_down`] on its own thread, as Windows does.
    fn watchdog_thread(
        max: Duration,
        keyed: &Arc<AtomicBool>,
        generation: &Arc<AtomicU64>,
    ) -> (
        mpsc::Sender<()>,
        std::thread::JoinHandle<bool>,
        Arc<AtomicBool>,
    ) {
        let (wake, cancel) = mpsc::channel();
        let released = Arc::new(AtomicBool::new(false));
        let (k, g, r) = (
            Arc::clone(keyed),
            Arc::clone(generation),
            Arc::clone(&released),
        );
        let mine = generation.load(Ordering::SeqCst);
        let thread = std::thread::spawn(move || {
            watch_key_down(&cancel, max, &k, &g, mine, || {
                r.store(true, Ordering::SeqCst)
            })
        });
        (wake, thread, released)
    }

    #[test]
    fn the_thread_watchdog_releases_an_overrunning_key_down_on_time() {
        let keyed = Arc::new(AtomicBool::new(true));
        let generation = Arc::new(AtomicU64::new(1));
        let started = Instant::now();
        let (_wake, thread, released) =
            watchdog_thread(Duration::from_millis(50), &keyed, &generation);
        assert!(thread.join().unwrap());
        assert!(released.load(Ordering::SeqCst));
        let took = started.elapsed();
        assert!(took >= Duration::from_millis(50), "fired early: {took:?}");
        assert!(
            took < Duration::from_millis(50) + PROMPT,
            "fired late: {took:?}"
        );
    }

    #[test]
    fn a_woken_thread_watchdog_stands_down_at_once_after_a_release() {
        let keyed = Arc::new(AtomicBool::new(true));
        let generation = Arc::new(AtomicU64::new(1));
        let (wake, thread, released) =
            watchdog_thread(Duration::from_secs(30), &keyed, &generation);
        keyed.store(false, Ordering::SeqCst);
        let started = Instant::now();
        wake.send(()).unwrap();
        assert!(!thread.join().unwrap());
        assert!(started.elapsed() < PROMPT);
        assert!(!released.load(Ordering::SeqCst));
    }

    #[test]
    fn a_woken_thread_watchdog_does_not_end_a_later_key_down() {
        let keyed = Arc::new(AtomicBool::new(true));
        let generation = Arc::new(AtomicU64::new(1));
        let (wake, thread, released) =
            watchdog_thread(Duration::from_secs(30), &keyed, &generation);
        generation.fetch_add(1, Ordering::SeqCst);
        wake.send(()).unwrap();
        assert!(!thread.join().unwrap());
        assert!(!released.load(Ordering::SeqCst));
    }

    #[test]
    fn a_spurious_wake_does_not_stand_the_watchdog_down() {
        let keyed = Arc::new(AtomicBool::new(true));
        let generation = Arc::new(AtomicU64::new(1));
        let (wake, thread, released) =
            watchdog_thread(Duration::from_millis(80), &keyed, &generation);
        wake.send(()).unwrap();
        assert!(
            thread.join().unwrap(),
            "still keyed: must fire at the deadline"
        );
        assert!(released.load(Ordering::SeqCst));
    }

    #[test]
    fn an_orphaned_thread_watchdog_still_fires_if_still_keyed() {
        // The `PttDtr` went away (sender dropped) but the disconnect
        // release did not clear `keyed`: the backstop must still fire.
        let keyed = Arc::new(AtomicBool::new(true));
        let generation = Arc::new(AtomicU64::new(1));
        let (wake, thread, released) =
            watchdog_thread(Duration::from_millis(80), &keyed, &generation);
        drop(wake);
        assert!(thread.join().unwrap());
        assert!(released.load(Ordering::SeqCst));
    }

    #[test]
    fn an_orphaned_thread_watchdog_exits_at_once_once_released() {
        // The usual disconnect: released synchronously, then the `PttDtr`
        // dropped. The thread must not hang on for `MAX_KEY_DOWN` holding a
        // broker handle.
        let keyed = Arc::new(AtomicBool::new(true));
        let generation = Arc::new(AtomicU64::new(1));
        let (wake, thread, released) =
            watchdog_thread(Duration::from_secs(30), &keyed, &generation);
        keyed.store(false, Ordering::SeqCst);
        let started = Instant::now();
        drop(wake);
        assert!(!thread.join().unwrap());
        assert!(started.elapsed() < PROMPT);
        assert!(!released.load(Ordering::SeqCst));
    }
}
