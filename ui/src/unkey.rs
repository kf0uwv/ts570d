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

//! Putting the radio back in receive, and *knowing* that it is.
//!
//! The console un-keys with CAT `RX;` in three places: the diagnostics
//! transmit step, the snapshot restore that ends every diagnostics run, and
//! the operator's own "PTT receive". Each used to send `RX;` once and throw
//! the result away (`let _ = radio.receive()`), so a lost or refused `RX;`
//! left the transmitter keyed with the console showing nothing wrong.
//!
//! [`ensure_receive`] sends `RX;`, then asks `IF;` whether the radio is
//! still transmitting, up to [`RX_CONFIRM_ATTEMPTS`] times. It does not
//! sleep between attempts: each one is already a full `RX;` + `IF;` round
//! trip on the wire, which is the time a radio needs to act on the first.
//! What it cannot confirm it reports, and the console keeps that report on
//! screen (see [`RECEIVE_ALERT_PREFIX`]) until a later receive is confirmed.

use radio::Radio;

/// How many `RX;` + `IF;` rounds before giving up and raising the alarm.
pub(crate) const RX_CONFIRM_ATTEMPTS: usize = 3;

/// What the console's message line starts with while the radio may still be
/// transmitting.
pub(crate) const RECEIVE_ALERT_PREFIX: &str = "!! RADIO MAY STILL BE TRANSMITTING";

/// The radio could not be confirmed back in receive.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "{RECEIVE_ALERT_PREFIX} -- receive not confirmed after {attempts} RX; attempts \
     (last: {last}). Check the radio and un-key it at the front panel."
)]
pub(crate) struct ReceiveUnconfirmed {
    /// How many `RX;` were sent.
    pub attempts: usize,
    /// Why the last attempt did not confirm.
    pub last: String,
}

/// Send `RX;` and confirm with `IF;` that the radio is receiving, retrying
/// up to [`RX_CONFIRM_ATTEMPTS`] times. Returns the attempts it took.
///
/// Logs every failed attempt, and an error if it gives up.
pub(crate) async fn ensure_receive<R: Radio + ?Sized>(
    radio: &mut R,
) -> Result<usize, ReceiveUnconfirmed> {
    let mut last = String::new();
    for attempt in 1..=RX_CONFIRM_ATTEMPTS {
        last = match radio.receive().await {
            Err(e) => format!("RX; failed: {e}"),
            Ok(()) => match radio.get_information().await {
                Ok(info) if !info.tx_rx => return Ok(attempt),
                Ok(_) => "IF; still reports transmit".to_string(),
                Err(e) => format!("IF; could not be read: {e}"),
            },
        };
        tracing::warn!("receive not confirmed (attempt {attempt}/{RX_CONFIRM_ATTEMPTS}): {last}");
    }
    let unconfirmed = ReceiveUnconfirmed {
        attempts: RX_CONFIRM_ATTEMPTS,
        last,
    };
    tracing::error!("{unconfirmed}");
    Err(unconfirmed)
}

/// Keep a standing receive alert on the message line, over whatever else
/// wrote there this frame.
pub(crate) fn stamp_receive_alert(message: &mut Option<String>, alert: Option<&str>) {
    if let Some(alert) = alert {
        if message.as_deref() != Some(alert) {
            *message = Some(alert.to_string());
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::VecDeque;

    use async_trait::async_trait;
    use futures::executor::block_on;
    use radio::{Frequency, InformationResponse, Mode, RadioError, RadioResult};

    use super::*;

    /// One `IF;` answer.
    pub(crate) fn info(transmitting: bool) -> InformationResponse {
        InformationResponse {
            frequency: Frequency::new(14_074_000).unwrap(),
            step: 0,
            rit_xit_offset: 0,
            rit_enabled: false,
            xit_enabled: false,
            memory_bank: 0,
            memory_channel: 0,
            tx_rx: transmitting,
            mode: Mode::Usb,
            vfo_memory: 0,
            scan_status: 0,
            split: false,
            ctcss_tone: 0,
            tone_number: 0,
        }
    }

    /// A radio whose `RX;` and `IF;` answers are scripted, and which counts
    /// what it was sent. Every other method keeps the trait's
    /// `NotImplemented` default.
    #[derive(Default)]
    pub(crate) struct MockRadio {
        /// `RX;` outcomes, in order; once empty, `RX;` succeeds.
        pub rx: VecDeque<RadioResult<()>>,
        /// `IF;` outcomes, in order; once empty, `IF;` says receiving.
        pub info: VecDeque<RadioResult<InformationResponse>>,
        /// `TX;` outcome; `Ok` if unset.
        pub tx: Option<RadioResult<()>>,
        pub rx_sent: usize,
        pub tx_sent: usize,
    }

    impl MockRadio {
        /// `RX;` always accepted, `IF;` always says still transmitting.
        pub(crate) fn stuck_in_transmit() -> Self {
            Self {
                info: (0..64).map(|_| Ok(info(true))).collect(),
                ..Self::default()
            }
        }
    }

    #[async_trait(?Send)]
    impl Radio for MockRadio {
        async fn transmit(&mut self) -> RadioResult<()> {
            self.tx_sent += 1;
            self.tx.take().unwrap_or(Ok(()))
        }
        async fn receive(&mut self) -> RadioResult<()> {
            self.rx_sent += 1;
            self.rx.pop_front().unwrap_or(Ok(()))
        }
        async fn get_information(&mut self) -> RadioResult<InformationResponse> {
            self.info.pop_front().unwrap_or_else(|| Ok(info(false)))
        }
    }

    #[test]
    fn a_radio_that_goes_to_receive_is_confirmed_first_time() {
        let mut radio = MockRadio::default();
        assert_eq!(block_on(ensure_receive(&mut radio)), Ok(1));
        assert_eq!(radio.rx_sent, 1);
    }

    #[test]
    fn a_failed_rx_is_retried() {
        let mut radio = MockRadio {
            rx: VecDeque::from([Err(RadioError::NotImplemented)]),
            ..MockRadio::default()
        };
        assert_eq!(block_on(ensure_receive(&mut radio)), Ok(2));
        assert_eq!(radio.rx_sent, 2);
    }

    #[test]
    fn an_rx_the_radio_did_not_act_on_is_retried() {
        // `RX;` accepted, but `IF;` says it is still transmitting.
        let mut radio = MockRadio {
            info: VecDeque::from([Ok(info(true)), Ok(info(true))]),
            ..MockRadio::default()
        };
        assert_eq!(block_on(ensure_receive(&mut radio)), Ok(3));
    }

    #[test]
    fn an_unreadable_if_does_not_count_as_confirmation() {
        let mut radio = MockRadio {
            info: VecDeque::from([Err(RadioError::NotImplemented)]),
            ..MockRadio::default()
        };
        assert_eq!(block_on(ensure_receive(&mut radio)), Ok(2));
    }

    #[test]
    fn a_radio_that_will_not_confirm_is_reported_after_a_bounded_number_of_tries() {
        let mut radio = MockRadio::stuck_in_transmit();
        let err = block_on(ensure_receive(&mut radio)).unwrap_err();
        assert_eq!(radio.rx_sent, RX_CONFIRM_ATTEMPTS);
        assert_eq!(err.attempts, RX_CONFIRM_ATTEMPTS);
        assert!(err.last.contains("still reports transmit"), "{err:?}");
        let shown = err.to_string();
        assert!(shown.starts_with(RECEIVE_ALERT_PREFIX), "{shown}");
    }

    #[test]
    fn the_report_names_the_last_cause() {
        let mut radio = MockRadio {
            rx: (0..RX_CONFIRM_ATTEMPTS)
                .map(|_| Err(RadioError::Link("server gone".into())))
                .collect(),
            ..MockRadio::default()
        };
        let err = block_on(ensure_receive(&mut radio)).unwrap_err();
        assert!(err.last.contains("RX; failed: server gone"), "{err:?}");
    }

    #[test]
    fn a_standing_alert_wins_the_message_line_every_frame() {
        let alert = "!! RADIO MAY STILL BE TRANSMITTING -- x";
        let mut message = Some("attached hw:2".to_string());
        stamp_receive_alert(&mut message, Some(alert));
        assert_eq!(message.as_deref(), Some(alert));

        // Something else writes the line the next frame; the alert returns.
        message = Some("spectrum: no frames".to_string());
        stamp_receive_alert(&mut message, Some(alert));
        assert_eq!(message.as_deref(), Some(alert));
    }

    #[test]
    fn no_alert_leaves_the_message_line_alone() {
        let mut message = Some("OK: PTT receive".to_string());
        stamp_receive_alert(&mut message, None);
        assert_eq!(message.as_deref(), Some("OK: PTT receive"));
    }
}
