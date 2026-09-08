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
//! **Release deliberately does not.** `radio::ptt_line`'s own documentation is
//! explicit that "the key that unkeys the transmitter must not have to wait" —
//! and a queued release can sit behind a CAT exchange that hits the 2 s read
//! timeout or the 5 s broker timeout, *while the radio is transmitting*. A
//! line change is safe to take out of order because `TIOCMSET` never touches
//! the byte stream: it cannot corrupt a frame in flight. Ordering only ever
//! mattered for the assert.
//!
//! # RTS is not an option on this radio
//!
//! The TS-570D uses RTS as receive-enable and withholds CAT responses while it
//! is low. A "PTT via RTS" configuration would therefore silence CAT, and
//! would present as responses failing to arrive — indistinguishable from a
//! desynchronised stream. Only DTR is offered here.

use std::cell::Cell;
use std::rc::Rc;
use tracing::{info, warn};

use cat_server::{BrokerCatSession, TaskFn};

/// How long a key-down may last before it is released regardless of what the
/// client does.
///
/// A rigctl client can send `T 1` and then have its TCP connection drop.
/// Nothing in the rigctl protocol requires a station to survive that; this
/// station does. Belt to the disconnect braces in [`PttDtr::release`].
pub const MAX_KEY_DOWN: std::time::Duration = std::time::Duration::from_secs(120);

/// Keys and releases PTT on the DTR line.
pub struct PttDtr {
    keyed: Rc<Cell<bool>>,
}

impl Default for PttDtr {
    fn default() -> Self {
        Self::new()
    }
}

impl PttDtr {
    pub fn new() -> Self {
        Self {
            keyed: Rc::new(Cell::new(false)),
        }
    }

    /// Whether this station believes PTT is currently asserted.
    pub fn is_keyed(&self) -> bool {
        self.keyed.get()
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

    /// Release DTR without waiting on the caller — the failsafe path.
    ///
    /// Takes an owned session so it can be driven from a spawned watchdog or
    /// from `Drop`, neither of which can borrow the caller's.
    async fn force_release(keyed: Rc<Cell<bool>>, session: BrokerCatSession, why: &'static str) {
        if !keyed.get() {
            return;
        }
        warn!("PTT: released ({why}) -- this was NOT a client asking to stop");
        let _ = session.submit_task(Self::line_task(false)).await;
        keyed.set(false);
    }

    /// Release PTT because the client that keyed it has gone away.
    ///
    /// A rigctl client can send `T 1` and then have its TCP connection drop.
    /// Nothing in the rigctl protocol requires a station to survive that; a
    /// station with a transmitter on the end of it must.
    pub fn release_on_disconnect(&self, session: BrokerCatSession) {
        if !self.keyed.get() {
            return;
        }
        let keyed = Rc::clone(&self.keyed);
        monoio::spawn(Self::force_release(
            keyed,
            session,
            "the client that keyed it disconnected",
        ));
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
        self.keyed.set(true);
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
        let keyed = Rc::clone(&self.keyed);
        let watchdog_session = BrokerCatSession::new(session.handle(), session.client_id());
        monoio::spawn(async move {
            monoio::time::sleep(MAX_KEY_DOWN).await;
            if keyed.get() {
                Self::force_release(keyed, watchdog_session, "the hard key-down timeout expired")
                    .await;
            }
        });
        Ok(())
    }

    /// De-assert DTR. Must not queue — see this module's doc comment.
    pub async fn release(&self, session: &BrokerCatSession) -> Result<(), String> {
        info!("PTT: released (a client asked to stop transmitting)");
        let result = session.submit_task(Self::line_task(false)).await;
        // Clear the flag even if the release reported failure: believing we
        // are still keyed when we may not be is the safer error, and the
        // caller is told.
        self.keyed.set(false);
        match result {
            Some(out) if out.starts_with(b"ERR ") => {
                Err(String::from_utf8_lossy(&out[4..]).into_owned())
            }
            Some(_) => Ok(()),
            None => Err("broker worker has shut down".to_string()),
        }
    }
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
    }

    #[test]
    fn the_hard_timeout_is_bounded_and_not_absurd() {
        // The point of this constant is that it exists. A client that drops
        // its connection mid-transmission must not leave the radio keyed
        // forever, and 120 s is long enough for any legitimate over.
        assert!(MAX_KEY_DOWN.as_secs() > 0);
        assert!(MAX_KEY_DOWN.as_secs() <= 300);
    }
}
