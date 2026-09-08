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

//! TS-570D controller-facing `Radio` trait and shared protocol domain types.
//!
//! This module defines the [`Radio`] trait that the `ui` and controller
//! consume, plus the TS-570D domain types that cross that boundary:
//! [`Frequency`], [`Mode`], [`InformationResponse`], [`MemoryChannelEntry`],
//! [`RadioError`], and [`RadioResult`].
//!
//! These types are TS-570D-specific (frequency range, mode encoding, `IF`
//! response layout) and therefore live in the `radio` crate, not in the
//! generic `framework`. `ui` depends on `radio` for them.

use std::fmt;

use async_trait::async_trait;
use cat_client::ClientError;
use cat_transport_core::TransportError;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that can occur during TS-570D CAT protocol operations.
#[derive(Debug, Error)]
pub enum RadioError {
    #[error("Invalid mode: {0}")]
    InvalidMode(u8),
    #[error("Frequency out of range: {0} Hz (valid: 500000–60000000)")]
    FrequencyOutOfRange(u64),
    #[error("Invalid protocol string: {0}")]
    InvalidProtocolString(String),
    #[error("Unknown command code: {0}")]
    UnknownCommand(String),
    #[error("Command {0} does not support read (query)")]
    CommandNotReadable(String),
    #[error("Command {0} does not support write (set)")]
    CommandNotWritable(String),
    #[error("Transport error: {0}")]
    Transport(#[from] TransportError),
    #[error("Not implemented")]
    NotImplemented,
    /// The link to the radio said something is wrong, in its own words.
    ///
    /// For a `Radio` that reaches its radio over a network protocol rather
    /// than a CAT session: a server that has not heard from the radio yet,
    /// a connection that dropped, a reply that does not fit this radio.
    /// None of those are CAT-level conditions, and forcing them into
    /// `InvalidProtocolString` would label a dead socket a parsing bug.
    ///
    /// The message is shown to an operator verbatim, so it should be the
    /// remote end's own words wherever there are any.
    #[error("{0}")]
    Link(String),
    #[error("Operation not supported")]
    Unsupported,
    /// The session is checked out by an in-flight CAT command.
    ///
    /// Only [`crate::ptt_line::PttLine`] can produce this: it is the one
    /// capability whose methods take `&self` and therefore run while the
    /// radio task may be holding the session across an `.await`. The caller's
    /// remedy is to try again in a few milliseconds, not to give up — see
    /// `radio/src/ptt_line.rs`.
    #[error("Radio session busy")]
    Busy,
}

/// Convenience [`Result`] alias for radio operations.
pub type RadioResult<T> = Result<T, RadioError>;

/// Reconciles `cat_client`'s generic client-side errors with this crate's
/// TS-570D-flavored [`RadioError`], at the boundary where `Ts570d` delegates
/// to `cat_client::CatClient` (see `radio/src/ts570d.rs`).
///
/// `ClientError::ProtocolError(kind)` maps to
/// [`RadioError::InvalidProtocolString`] with the exact same message text
/// `RadioClient::execute_query` used to produce before this crate depended on
/// `cat_client` — this is a relocation of that mapping, not a behavior
/// change.
impl From<ClientError<TransportError>> for RadioError {
    fn from(err: ClientError<TransportError>) -> Self {
        match err {
            ClientError::UnknownCommand(code) => RadioError::UnknownCommand(code),
            ClientError::CommandNotReadable(code) => RadioError::CommandNotReadable(code),
            ClientError::CommandNotWritable(code) => RadioError::CommandNotWritable(code),
            ClientError::ProtocolError(kind) => RadioError::InvalidProtocolString(format!(
                "session reported a protocol error: {:?}",
                kind
            )),
            ClientError::Transport(e) => RadioError::Transport(e),
        }
    }
}

// ---------------------------------------------------------------------------
// Frequency
// ---------------------------------------------------------------------------

/// Frequency in Hertz, validated to the TS-570D receive range (500 kHz–60 MHz).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Frequency(u64);

impl Frequency {
    pub const MIN_HZ: u64 = 500_000;
    pub const MAX_HZ: u64 = 60_000_000;

    /// Construct a `Frequency`, returning [`RadioError::FrequencyOutOfRange`]
    /// if `hz` is outside the valid range.
    pub fn new(hz: u64) -> Result<Self, RadioError> {
        if !(Self::MIN_HZ..=Self::MAX_HZ).contains(&hz) {
            return Err(RadioError::FrequencyOutOfRange(hz));
        }
        Ok(Frequency(hz))
    }

    /// Return the raw frequency in Hz.
    pub fn hz(self) -> u64 {
        self.0
    }

    /// Format as an 11-digit zero-padded protocol string, e.g. `"00014230000"`.
    pub fn to_protocol_string(self) -> String {
        format!("{:011}", self.0)
    }

    /// Parse from an 11-digit protocol string.
    pub fn from_protocol_str(s: &str) -> Result<Self, RadioError> {
        let hz = s
            .parse::<u64>()
            .map_err(|_| RadioError::InvalidProtocolString(s.to_string()))?;
        Self::new(hz)
    }
}

impl fmt::Display for Frequency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mhz = self.0 as f64 / 1_000_000.0;
        write!(f, "{:.3} MHz", mhz)
    }
}

// ---------------------------------------------------------------------------
// Mode
// ---------------------------------------------------------------------------

/// TS-570D operating modes per CAT protocol specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Mode {
    Lsb = 1,
    Usb = 2,
    Cw = 3,
    Fm = 4,
    Am = 5,
    Fsk = 6,
    CwReverse = 7,
    // 8 is not used on TS-570D
    FskReverse = 9,
}

impl Mode {
    /// Return the numeric byte value used in the CAT protocol.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Return the human-readable name string.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Lsb => "LSB",
            Mode::Usb => "USB",
            Mode::Cw => "CW",
            Mode::Fm => "FM",
            Mode::Am => "AM",
            Mode::Fsk => "FSK",
            Mode::CwReverse => "CW-R",
            Mode::FskReverse => "FSK-R",
        }
    }
}

impl TryFrom<u8> for Mode {
    type Error = RadioError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Mode::Lsb),
            2 => Ok(Mode::Usb),
            3 => Ok(Mode::Cw),
            4 => Ok(Mode::Fm),
            5 => Ok(Mode::Am),
            6 => Ok(Mode::Fsk),
            7 => Ok(Mode::CwReverse),
            9 => Ok(Mode::FskReverse),
            _ => Err(RadioError::InvalidMode(value)),
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ---------------------------------------------------------------------------
// InformationResponse
// ---------------------------------------------------------------------------

/// Composite response to the `IF` (Information) query.
///
/// The TS-570D packs the full operational state into a single 37-character
/// payload (plus command code and terminator).
///
/// Wire layout (chars after `IF`, before `;`):
/// ```text
/// Pos  Len  Field
///   0   11  frequency (Hz, zero-padded)
///  11    4  step (Hz, 4-digit)
///  15    5  RIT/XIT offset (-9999 to +9999 Hz, signed, leading sign)
///  20    1  RIT enabled (0/1)
///  21    1  XIT enabled (0/1)
///  22    2  memory bank (00–09)
///  24    2  memory channel (00–99)
///  26    1  TX/RX status (0=RX, 1=TX)
///  27    1  mode (1–9)
///  28    1  VFO/memory (0=VFO, 1=Memory)
///  29    1  scan status (0=off, 1=on)
///  30    1  split (0=off, 1=on)
///  31    2  CTCSS tone number (00–42)
///  33    2  tone number (00–42)
///  35    1  offset indicator (always 0 on TS-570D)
///  36    1  reserved
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct InformationResponse {
    /// Current VFO frequency in Hz.
    pub frequency: Frequency,
    /// Tuning step in Hz.
    pub step: u32,
    /// RIT/XIT offset in Hz (signed).
    pub rit_xit_offset: i32,
    /// RIT enabled.
    pub rit_enabled: bool,
    /// XIT enabled.
    pub xit_enabled: bool,
    /// Memory bank (0–9; 0 when in VFO mode).
    pub memory_bank: u8,
    /// Memory channel (0–99; 0 when in VFO mode).
    pub memory_channel: u8,
    /// `true` = transmitting.
    pub tx_rx: bool,
    /// Operating mode.
    pub mode: Mode,
    /// `0` = VFO mode, `1` = memory mode.
    pub vfo_memory: u8,
    /// Scan status (`0` = off, `1` = scanning).
    pub scan_status: u8,
    /// Split operation enabled.
    pub split: bool,
    /// CTCSS tone number (0–42).
    pub ctcss_tone: u8,
    /// Tone number (0–42).
    pub tone_number: u8,
}

// ---------------------------------------------------------------------------
// MemoryChannelEntry
// ---------------------------------------------------------------------------

/// Contents of a single memory channel (MR/MW commands).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryChannelEntry {
    /// Channel number 0–99.
    pub channel: u8,
    /// `true` if this is a split-TX channel.
    pub split: bool,
    /// Frequency in Hz. `0` means vacant.
    pub freq_hz: u64,
    /// Mode digit: 1=LSB 2=USB 3=CW 4=FM 5=AM 6=FSK 7=CW-R 9=FSK-R. `0` means vacant.
    pub mode: u8,
    /// Channel lockout active.
    pub lockout: bool,
    /// Tone type: 0=off, 1=tone, 2=CTCSS.
    pub tone_type: u8,
    /// Tone number 0–39.
    pub tone_number: u8,
    /// `true` if the channel is vacant (freq == 0).
    pub vacant: bool,
}

// ---------------------------------------------------------------------------
// Radio trait
// ---------------------------------------------------------------------------

/// Abstraction over a TS-570D (or compatible) radio.
///
/// Implemented by `radio::Ts570d<S: CatSession>`.  UI and other framework
/// consumers depend only on this trait, not on the concrete radio crate.
///
/// Uses `#[async_trait(?Send)]` — no `Send` bounds, compatible with monoio's
/// thread-per-core (`!Send`) futures.
#[async_trait(?Send)]
pub trait Radio {
    async fn get_vfo_a(&mut self) -> RadioResult<Frequency> {
        Err(RadioError::NotImplemented)
    }
    async fn set_vfo_a(&mut self, _freq: Frequency) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    async fn get_vfo_b(&mut self) -> RadioResult<Frequency> {
        Err(RadioError::NotImplemented)
    }
    async fn set_vfo_b(&mut self, _freq: Frequency) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    async fn get_mode(&mut self) -> RadioResult<Mode> {
        Err(RadioError::NotImplemented)
    }
    async fn set_mode(&mut self, _mode: Mode) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    async fn get_smeter(&mut self) -> RadioResult<u16> {
        Err(RadioError::NotImplemented)
    }
    /// The selected transmit meter and its reading: `(selector, value)`.
    ///
    /// A different meter from [`Self::get_smeter`]. On a TS-570D `SM;`
    /// answers with signal strength while receiving and, per the manual,
    /// "a calibrated power meter" while transmitting; `RM;` answers with
    /// whichever of SWR, compression or ALC the operator has selected.
    /// The selector is `0` none, `1` SWR, `2` COMP, `3` ALC.
    ///
    /// Zero while receiving -- all three are transmit meters.
    async fn get_meter_reading(&mut self) -> RadioResult<(u8, u16)> {
        Err(RadioError::NotImplemented)
    }
    async fn transmit(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    async fn receive(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    async fn get_id(&mut self) -> RadioResult<u16> {
        Err(RadioError::NotImplemented)
    }
    async fn get_information(&mut self) -> RadioResult<InformationResponse> {
        Err(RadioError::NotImplemented)
    }
    async fn get_af_gain(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    async fn set_af_gain(&mut self, _level: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    async fn get_rf_gain(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    async fn set_rf_gain(&mut self, _level: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    async fn get_power(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    async fn set_power(&mut self, _watts: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Receive / signal chain
    // -----------------------------------------------------------------------

    /// Get noise blanker state.
    async fn get_noise_blanker(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Set noise blanker on/off.
    async fn set_noise_blanker(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Get noise reduction level (0=off, 1=NR1, 2=NR2).
    async fn get_noise_reduction(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set noise reduction level.
    async fn set_noise_reduction(&mut self, _level: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Get pre-amplifier state.
    async fn get_preamp(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Set pre-amplifier on/off.
    async fn set_preamp(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Get attenuator state.
    async fn get_attenuator(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Set attenuator on/off.
    async fn set_attenuator(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Get squelch level (0–255).
    async fn get_squelch(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set squelch level.
    async fn set_squelch(&mut self, _level: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Get microphone gain (0–100).
    async fn get_mic_gain(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set microphone gain.
    async fn set_mic_gain(&mut self, _gain: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Get AGC time constant (2=fast, 4=slow).
    async fn get_agc(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set AGC time constant.
    async fn set_agc(&mut self, _constant: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // RIT / XIT / Scan
    // -----------------------------------------------------------------------

    /// Get RIT on/off state.
    async fn get_rit(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Set RIT on/off.
    async fn set_rit(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Clear RIT/XIT offset to zero (RC command).
    async fn clear_rit(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Increment RIT/XIT offset (RU command).
    async fn rit_up(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Decrement RIT/XIT offset (RD command).
    async fn rit_down(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Get XIT on/off state.
    async fn get_xit(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Set XIT on/off.
    async fn set_xit(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Get scan on/off state.
    async fn get_scan(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Set scan on/off.
    async fn set_scan(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // VOX
    // -----------------------------------------------------------------------

    /// Get VOX on/off state.
    async fn get_vox(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Set VOX on/off.
    async fn set_vox(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Get VOX gain (1–9).
    async fn get_vox_gain(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set VOX gain.
    async fn set_vox_gain(&mut self, _gain: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Get VOX delay in milliseconds (0–3000).
    async fn get_vox_delay(&mut self) -> RadioResult<u16> {
        Err(RadioError::NotImplemented)
    }
    /// Set VOX delay in milliseconds.
    async fn set_vox_delay(&mut self, _ms: u16) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Frequency utilities
    // -----------------------------------------------------------------------

    /// Get receiver VFO/memory selection (0=VFO A, 1=VFO B, 2=Memory).
    async fn get_rx_vfo(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set receiver VFO/memory selection.
    async fn set_rx_vfo(&mut self, _vfo: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Get transmitter VFO/memory selection (0=VFO A, 1=VFO B, 2=Memory).
    async fn get_tx_vfo(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set transmitter VFO/memory selection.
    async fn set_tx_vfo(&mut self, _vfo: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Get frequency lock state.
    async fn get_frequency_lock(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Set frequency lock on/off.
    async fn set_frequency_lock(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Power and status
    // -----------------------------------------------------------------------

    /// Get transceiver power on/off state.
    async fn get_power_on(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Set transceiver power on/off.
    async fn set_power_on(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Check if receiver is busy (carrier detected).
    async fn is_busy(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Get speech processor on/off state.
    async fn get_speech_processor(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Set speech processor on/off.
    async fn set_speech_processor(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Memory
    // -----------------------------------------------------------------------

    /// Get current memory channel number (0–99).
    async fn get_memory_channel(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set memory channel number.
    async fn set_memory_channel(&mut self, _ch: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Read memory channel contents (MR command).
    async fn read_memory_channel(&mut self, _ch: u8) -> RadioResult<MemoryChannelEntry> {
        Err(RadioError::Unsupported)
    }

    /// Write memory channel contents (MW command).
    /// `entry.channel` must match `ch`.
    async fn write_memory_channel(
        &mut self,
        _ch: u8,
        _entry: MemoryChannelEntry,
    ) -> RadioResult<()> {
        Err(RadioError::Unsupported)
    }

    /// Clear a memory channel (write a vacant entry).
    async fn clear_memory_channel(&mut self, _ch: u8) -> RadioResult<()> {
        Err(RadioError::Unsupported)
    }

    // -----------------------------------------------------------------------
    // Antenna
    // -----------------------------------------------------------------------

    /// Get antenna selection (1=ANT1, 2=ANT2).
    async fn get_antenna(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set antenna selection.
    async fn set_antenna(&mut self, _ant: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // CW keyer
    // -----------------------------------------------------------------------

    /// Send a CW message via the keyer buffer (KY command, up to 24 chars).
    async fn send_cw(&mut self, _message: &str) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Get keyer speed in WPM (10–60).
    async fn get_keyer_speed(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set keyer speed in WPM (10–60).
    async fn set_keyer_speed(&mut self, _wpm: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Get CW pitch index (00–12 maps to 400–1000 Hz).
    async fn get_cw_pitch(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set CW pitch index (00–12).
    async fn set_cw_pitch(&mut self, _pitch: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Antenna tuner
    // -----------------------------------------------------------------------

    /// Set antenna tuner to through (bypass) or tuner mode.
    async fn set_antenna_tuner_thru(&mut self, _thru: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Start antenna tuning cycle.
    async fn start_antenna_tuning(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // DSP slope filters
    // -----------------------------------------------------------------------

    /// Get high cutoff filter index (SH, 00–20).
    async fn get_high_cutoff(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set high cutoff filter index (00–20).
    async fn set_high_cutoff(&mut self, _val: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Get low cutoff filter index (SL, 00–20).
    async fn get_low_cutoff(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set low cutoff filter index (00–20).
    async fn set_low_cutoff(&mut self, _val: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Tone / CTCSS
    // -----------------------------------------------------------------------

    /// Get CTCSS tone number (01–39).
    async fn get_ctcss_tone_number(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set CTCSS tone number (01–39).
    async fn set_ctcss_tone_number(&mut self, _n: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Get CTCSS on/off state.
    async fn get_ctcss(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Set CTCSS on/off.
    async fn set_ctcss(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Get tone number (01–39).
    async fn get_tone_number(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set tone number (01–39).
    async fn set_tone_number(&mut self, _n: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Get tone on/off state.
    async fn get_tone(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Set tone on/off.
    async fn set_tone(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Beat cancel
    // -----------------------------------------------------------------------

    /// Get beat cancel mode (0=off, 1=on, 2=enhanced).
    async fn get_beat_cancel(&mut self) -> RadioResult<u8> {
        Err(RadioError::NotImplemented)
    }
    /// Set beat cancel mode (0=off, 1=on, 2=enhanced).
    async fn set_beat_cancel(&mut self, _mode: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // IF shift
    // -----------------------------------------------------------------------

    /// Get IF shift direction and frequency offset.
    async fn get_if_shift(&mut self) -> RadioResult<(char, u16)> {
        Err(RadioError::NotImplemented)
    }
    /// Set IF shift direction and frequency offset.
    async fn set_if_shift(&mut self, _direction: char, _freq: u16) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Voice synthesizer
    // -----------------------------------------------------------------------

    /// Recall voice message (1 or 2).
    async fn voice_recall(&mut self, _voice: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // System reset
    // -----------------------------------------------------------------------

    /// Reset the transceiver (false=partial, true=full).
    async fn reset(&mut self, _full: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Meter reading
    // -----------------------------------------------------------------------

    /// Read meter value (1=SWR, 2=COMP, 3=ALC).
    async fn get_meter(&mut self, _meter: u8) -> RadioResult<u16> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Semi break-in delay
    // -----------------------------------------------------------------------

    /// Get semi break-in delay in ms (0–1000, 50ms steps).
    async fn get_semi_break_in_delay(&mut self) -> RadioResult<u16> {
        Err(RadioError::NotImplemented)
    }
    /// Set semi break-in delay in ms (0–1000, 50ms steps).
    async fn set_semi_break_in_delay(&mut self, _ms: u16) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // CW auto zero-beat
    // -----------------------------------------------------------------------

    /// Get CW auto zero-beat state.
    async fn get_cw_auto_zerobeat(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Set CW auto zero-beat on/off.
    async fn set_cw_auto_zerobeat(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Fine step
    // -----------------------------------------------------------------------

    /// Get fine step state.
    async fn get_fine_step(&mut self) -> RadioResult<bool> {
        Err(RadioError::NotImplemented)
    }
    /// Set fine step on/off.
    async fn set_fine_step(&mut self, _on: bool) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // Auto information
    // -----------------------------------------------------------------------

    /// Set auto information mode (0=off, 1–3=various levels).
    async fn set_auto_info(&mut self, _mode: u8) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    // -----------------------------------------------------------------------
    // MIC up/down (write-only momentary)
    // -----------------------------------------------------------------------

    /// Send MIC Up command.
    async fn mic_up(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }
    /// Send MIC Down command.
    async fn mic_down(&mut self) -> RadioResult<()> {
        Err(RadioError::NotImplemented)
    }

    /// Flush the receive buffer, discarding unsolicited or stale data.
    /// Default implementation is a no-op.
    fn flush_rx(&mut self) {}
}

// ---------------------------------------------------------------------------
// NopRadio
// ---------------------------------------------------------------------------

/// A no-op [`Radio`] implementation. All methods return [`RadioError::NotImplemented`].
/// Useful as a starting point — implement only the methods your radio supports.
pub struct NopRadio;

#[async_trait(?Send)]
impl Radio for NopRadio {}

// ---------------------------------------------------------------------------
// Tests (moved from radio/src/protocol/frequency.rs and mode.rs)
// ---------------------------------------------------------------------------

// Gated to Linux: this module's test(s) use #[monoio::test] (see
// docs/adr/0006-windows-concurrency-model.md).
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    // --- Frequency tests ---

    #[test]
    fn test_valid_frequency() {
        let freq = Frequency::new(14_230_000).unwrap();
        assert_eq!(freq.hz(), 14_230_000);
    }

    #[test]
    fn test_boundary_min() {
        let freq = Frequency::new(Frequency::MIN_HZ).unwrap();
        assert_eq!(freq.hz(), 500_000);
    }

    #[test]
    fn test_boundary_max() {
        let freq = Frequency::new(Frequency::MAX_HZ).unwrap();
        assert_eq!(freq.hz(), 60_000_000);
    }

    #[test]
    fn test_out_of_range_below() {
        let result = Frequency::new(499_999);
        assert!(result.is_err());
        match result.unwrap_err() {
            RadioError::FrequencyOutOfRange(v) => assert_eq!(v, 499_999),
            _ => panic!("expected FrequencyOutOfRange"),
        }
    }

    #[test]
    fn test_out_of_range_above() {
        let result = Frequency::new(60_000_001);
        assert!(result.is_err());
        match result.unwrap_err() {
            RadioError::FrequencyOutOfRange(v) => assert_eq!(v, 60_000_001),
            _ => panic!("expected FrequencyOutOfRange"),
        }
    }

    #[test]
    fn test_out_of_range_zero() {
        let result = Frequency::new(0);
        assert!(result.is_err());
    }

    #[test]
    fn test_protocol_string_format() {
        let freq = Frequency::new(14_230_000).unwrap();
        assert_eq!(freq.to_protocol_string(), "00014230000");
    }

    #[test]
    fn test_protocol_string_min() {
        let freq = Frequency::new(500_000).unwrap();
        assert_eq!(freq.to_protocol_string(), "00000500000");
    }

    #[test]
    fn test_protocol_string_max() {
        let freq = Frequency::new(60_000_000).unwrap();
        assert_eq!(freq.to_protocol_string(), "00060000000");
    }

    #[test]
    fn test_from_protocol_str_round_trip() {
        let freq = Frequency::new(14_230_000).unwrap();
        let s = freq.to_protocol_string();
        let recovered = Frequency::from_protocol_str(&s).unwrap();
        assert_eq!(recovered, freq);
    }

    #[test]
    fn test_from_protocol_str_invalid() {
        let result = Frequency::from_protocol_str("not_a_number");
        assert!(result.is_err());
        match result.unwrap_err() {
            RadioError::InvalidProtocolString(_) => {}
            _ => panic!("expected InvalidProtocolString"),
        }
    }

    #[test]
    fn test_from_protocol_str_out_of_range() {
        let result = Frequency::from_protocol_str("00000000001");
        assert!(result.is_err());
        match result.unwrap_err() {
            RadioError::FrequencyOutOfRange(_) => {}
            _ => panic!("expected FrequencyOutOfRange"),
        }
    }

    #[test]
    fn test_display_format() {
        let freq = Frequency::new(14_230_000).unwrap();
        assert_eq!(freq.to_string(), "14.230 MHz");
    }

    #[test]
    fn test_display_format_min() {
        let freq = Frequency::new(500_000).unwrap();
        assert_eq!(freq.to_string(), "0.500 MHz");
    }

    #[test]
    fn test_ordering() {
        let low = Frequency::new(7_000_000).unwrap();
        let high = Frequency::new(14_000_000).unwrap();
        assert!(low < high);
        assert!(high > low);
        assert_eq!(low, low);
    }

    // --- Mode tests ---

    #[test]
    fn test_all_valid_modes() {
        assert_eq!(Mode::try_from(1).unwrap(), Mode::Lsb);
        assert_eq!(Mode::try_from(2).unwrap(), Mode::Usb);
        assert_eq!(Mode::try_from(3).unwrap(), Mode::Cw);
        assert_eq!(Mode::try_from(4).unwrap(), Mode::Fm);
        assert_eq!(Mode::try_from(5).unwrap(), Mode::Am);
        assert_eq!(Mode::try_from(6).unwrap(), Mode::Fsk);
        assert_eq!(Mode::try_from(7).unwrap(), Mode::CwReverse);
        assert_eq!(Mode::try_from(9).unwrap(), Mode::FskReverse);
    }

    #[test]
    fn test_invalid_mode_zero() {
        let result = Mode::try_from(0u8);
        assert!(result.is_err());
        match result.unwrap_err() {
            RadioError::InvalidMode(v) => assert_eq!(v, 0),
            _ => panic!("expected InvalidMode"),
        }
    }

    #[test]
    fn test_invalid_mode_eight() {
        let result = Mode::try_from(8u8);
        assert!(result.is_err());
        match result.unwrap_err() {
            RadioError::InvalidMode(v) => assert_eq!(v, 8),
            _ => panic!("expected InvalidMode"),
        }
    }

    #[test]
    fn test_invalid_mode_ten() {
        let result = Mode::try_from(10u8);
        assert!(result.is_err());
        match result.unwrap_err() {
            RadioError::InvalidMode(v) => assert_eq!(v, 10),
            _ => panic!("expected InvalidMode"),
        }
    }

    #[test]
    fn test_mode_display() {
        assert_eq!(Mode::Lsb.to_string(), "LSB");
        assert_eq!(Mode::Usb.to_string(), "USB");
        assert_eq!(Mode::Cw.to_string(), "CW");
        assert_eq!(Mode::Fm.to_string(), "FM");
        assert_eq!(Mode::Am.to_string(), "AM");
        assert_eq!(Mode::Fsk.to_string(), "FSK");
        assert_eq!(Mode::CwReverse.to_string(), "CW-R");
        assert_eq!(Mode::FskReverse.to_string(), "FSK-R");
    }

    #[test]
    fn test_mode_round_trip() {
        let modes = [
            Mode::Lsb,
            Mode::Usb,
            Mode::Cw,
            Mode::Fm,
            Mode::Am,
            Mode::Fsk,
            Mode::CwReverse,
            Mode::FskReverse,
        ];
        for mode in modes {
            let byte = mode.as_u8();
            let recovered = Mode::try_from(byte).expect("round-trip should succeed");
            assert_eq!(recovered, mode);
        }
    }

    #[test]
    fn test_mode_as_u8_values() {
        assert_eq!(Mode::Lsb.as_u8(), 1);
        assert_eq!(Mode::Usb.as_u8(), 2);
        assert_eq!(Mode::Cw.as_u8(), 3);
        assert_eq!(Mode::Fm.as_u8(), 4);
        assert_eq!(Mode::Am.as_u8(), 5);
        assert_eq!(Mode::Fsk.as_u8(), 6);
        assert_eq!(Mode::CwReverse.as_u8(), 7);
        assert_eq!(Mode::FskReverse.as_u8(), 9);
    }

    // --- NopRadio tests ---

    #[monoio::test(driver = "legacy")]
    async fn test_nop_radio_returns_not_implemented() {
        let mut radio = NopRadio;
        assert!(matches!(
            radio.get_vfo_a().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.set_vfo_a(Frequency::new(14_000_000).unwrap()).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_mode().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.transmit().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_smeter().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.reset(false).await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.send_cw("CQ").await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_if_shift().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_information().await,
            Err(RadioError::NotImplemented)
        ));
        assert!(matches!(
            radio.get_vox_delay().await,
            Err(RadioError::NotImplemented)
        ));
    }

    // --- InformationResponse tests ---

    #[test]
    fn test_information_response_fields() {
        let info = InformationResponse {
            frequency: Frequency::new(14_230_000).unwrap(),
            step: 1000,
            rit_xit_offset: -500,
            rit_enabled: true,
            xit_enabled: false,
            memory_bank: 0,
            memory_channel: 0,
            tx_rx: false,
            mode: Mode::Usb,
            vfo_memory: 0,
            scan_status: 0,
            split: false,
            ctcss_tone: 0,
            tone_number: 0,
        };
        assert_eq!(info.frequency, Frequency::new(14_230_000).unwrap());
        assert_eq!(info.mode, Mode::Usb);
        assert!(info.rit_enabled);
        assert!(!info.xit_enabled);
        assert_eq!(info.rit_xit_offset, -500);
    }
}

// ---------------------------------------------------------------------------
// Audio passband
// ---------------------------------------------------------------------------

/// The receive audio passband for `mode`, as offsets from the dial in Hz.
///
/// Signed, so the **sideband is part of the answer**: USB hears what is
/// above the dial, LSB what is below, and a console that got that backwards
/// would draw an AF display that is confidently wrong.
///
/// Lives here, in the radio crate, because it is a fact about a TS-570D and
/// two very different things need it: the emulator, to decide which signals
/// reach ACC2 pin 3, and the console, to mark the passband edges on its AF
/// FFT. Those two must agree — an operator looking at a tone outside the
/// marked edges and hearing it anyway is being told a lie by one of them —
/// and the only way to guarantee they agree is for there to be one of these.
///
/// `cw_pitch` is the menu index, not a frequency: the TS-570D offers
/// 400-1000 Hz in **50 Hz** steps, so index 0 is 400 Hz and index 12 is
/// 1000 Hz. Menu 20 lists all thirteen (400 450 500 ... 1000), and CAT
/// parameter format 52 states "00 (400 Hz min.) ~ 12 (1000 Hz max.)" --
/// which only holds at 50 Hz per step. This used to read 100, which put
/// the CW window as much as 600 Hz above the tone at the top of the
/// range, and the marked filter edges disagreed with what you could hear.
pub fn audio_passband_hz(mode: Mode, cw_pitch: u8) -> (f32, f32) {
    /// The low edge of a voice passband. Below this is rumble, and the
    /// radio does not pass it.
    const AF_LOW: f32 = 300.0;
    const AF_HIGH: f32 = 2_700.0;

    let pitch = 400.0 + f32::from(cw_pitch) * 50.0;
    match mode {
        Mode::Lsb | Mode::FskReverse => (-AF_HIGH, -AF_LOW),
        Mode::Usb | Mode::Fsk => (AF_LOW, AF_HIGH),
        // A narrow window placed at the sidetone pitch, on the side the
        // mode listens to.
        Mode::Cw => (pitch - 250.0, pitch + 250.0),
        Mode::CwReverse => (-pitch - 250.0, -pitch + 250.0),
        // Double-sideband, and both carry audio through zero beat.
        Mode::Am => (-4_500.0, 4_500.0),
        Mode::Fm => (-5_000.0, 5_000.0),
    }
}

#[cfg(test)]
mod passband_tests {
    use super::*;

    #[test]
    fn usb_listens_above_the_dial_and_lsb_below() {
        // The one that matters. Getting the sideband backwards makes an AF
        // display wrong in a way that looks fine.
        let (lo, hi) = audio_passband_hz(Mode::Usb, 0);
        assert!(lo > 0.0 && hi > lo);
        let (lo, hi) = audio_passband_hz(Mode::Lsb, 0);
        assert!(hi < 0.0 && lo < hi);
    }

    #[test]
    fn cw_listens_at_the_sidetone_pitch_and_moves_with_it() {
        let (lo, hi) = audio_passband_hz(Mode::Cw, 3); // index 3 -> 700 Hz
        assert!(lo < 700.0 && hi > 700.0);
        let (lo2, _) = audio_passband_hz(Mode::Cw, 0); // 400 Hz
        assert!(lo2 < lo, "a lower pitch moves the window down");
    }

    #[test]
    fn cw_reverse_is_the_same_window_on_the_other_side() {
        let (lo, hi) = audio_passband_hz(Mode::Cw, 3);
        let (rlo, rhi) = audio_passband_hz(Mode::CwReverse, 3);
        assert!((rlo + hi).abs() < f32::EPSILON);
        assert!((rhi + lo).abs() < f32::EPSILON);
    }

    #[test]
    fn the_double_sideband_modes_pass_through_zero_beat() {
        for mode in [Mode::Am, Mode::Fm] {
            let (lo, hi) = audio_passband_hz(mode, 0);
            assert!(lo < 0.0 && hi > 0.0, "{mode} must span zero");
        }
    }

    #[test]
    fn every_mode_has_a_passband_and_none_is_inverted() {
        for mode in [
            Mode::Lsb,
            Mode::Usb,
            Mode::Cw,
            Mode::Fm,
            Mode::Am,
            Mode::Fsk,
            Mode::CwReverse,
            Mode::FskReverse,
        ] {
            let (lo, hi) = audio_passband_hz(mode, 0);
            assert!(lo < hi, "{mode}: {lo} .. {hi} is inverted");
        }
    }
}
