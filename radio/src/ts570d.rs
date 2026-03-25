//! Typed TS-570D radio client.
//!
//! [`Ts570d`] wraps a [`RadioClient`] and provides strongly-typed convenience
//! methods for every common radio operation.  Each getter queries the radio and
//! parses the response into the appropriate Rust type; each setter formats the
//! typed value back into a CAT parameter string before delegating to
//! [`RadioClient::set`].
//!
//! # Wire formats used
//!
//! | Operation       | Code | Query/Set format               |
//! |-----------------|------|--------------------------------|
//! | VFO A frequency | FA   | `FA;` / `FA<11 digits>;`       |
//! | VFO B frequency | FB   | `FB;` / `FB<11 digits>;`       |
//! | Mode            | MD   | `MD;` / `MD<digit>;`           |
//! | S-meter         | SM   | `SM0;` / read-only             |
//! | PTT transmit    | TX   | `TX;` / write-only             |
//! | PTT receive     | RX   | `RX;` / write-only             |
//! | Radio ID        | ID   | `ID;` / read-only              |
//! | IF information  | IF   | `IF;` / read-only              |
//! | AF gain         | AG   | `AG;` / `AG0<3 digits>;`       |
//! | RF gain         | RG   | `RG;` / `RG<3 digits>;`        |
//! | TX power        | PC   | `PC;` / `PC<3 digits>;`        |

use framework::transport::Transport;

use framework::radio::{Frequency, MemoryChannelEntry, Mode, RadioError, RadioResult};

use crate::client::RadioClient;
use crate::protocol::{InformationResponse, Response, ResponseParser};

/// Strongly-typed client for the Kenwood TS-570D CAT interface.
///
/// Wraps [`RadioClient<T>`] and converts raw protocol strings into typed Rust
/// values.  All async methods are monoio-compatible (`!Send`).
pub struct Ts570d<T: Transport> {
    pub(crate) client: RadioClient<T>,
}

impl<T: Transport> Ts570d<T> {
    /// Create a new `Ts570d` wrapping the given transport.
    pub fn new(transport: T) -> Self {
        Self {
            client: RadioClient::new(transport),
        }
    }

    // -----------------------------------------------------------------------
    // VFO A frequency
    // -----------------------------------------------------------------------

    /// Query the current VFO A frequency.
    pub async fn get_vfo_a(&mut self) -> RadioResult<Frequency> {
        let raw = self.client.query("FA").await?;
        match ResponseParser::parse(&raw)? {
            Response::VfoAFrequency(freq) => Ok(freq),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected VfoAFrequency, got {:?}",
                other
            ))),
        }
    }

    /// Set the VFO A frequency.
    pub async fn set_vfo_a(&mut self, freq: Frequency) -> RadioResult<()> {
        self.client.set("FA", &freq.to_protocol_string()).await
    }

    // -----------------------------------------------------------------------
    // VFO B frequency
    // -----------------------------------------------------------------------

    /// Query the current VFO B frequency.
    pub async fn get_vfo_b(&mut self) -> RadioResult<Frequency> {
        let raw = self.client.query("FB").await?;
        match ResponseParser::parse(&raw)? {
            Response::VfoBFrequency(freq) => Ok(freq),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected VfoBFrequency, got {:?}",
                other
            ))),
        }
    }

    /// Set the VFO B frequency.
    pub async fn set_vfo_b(&mut self, freq: Frequency) -> RadioResult<()> {
        self.client.set("FB", &freq.to_protocol_string()).await
    }

    // -----------------------------------------------------------------------
    // Operating mode
    // -----------------------------------------------------------------------

    /// Query the current operating mode.
    pub async fn get_mode(&mut self) -> RadioResult<Mode> {
        let raw = self.client.query("MD").await?;
        match ResponseParser::parse(&raw)? {
            Response::Mode(mode) => Ok(mode),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected Mode, got {:?}",
                other
            ))),
        }
    }

    /// Set the operating mode.
    pub async fn set_mode(&mut self, mode: Mode) -> RadioResult<()> {
        self.client.set("MD", &format!("{}", mode.as_u8())).await
    }

    // -----------------------------------------------------------------------
    // S-meter (read-only)
    // -----------------------------------------------------------------------

    /// Query the S-meter reading (main receiver).
    ///
    /// Returns the raw 0–15 reading from the `SM` response.
    /// Manual p.80, format 22: SM value range is 0000–0015.
    pub async fn get_smeter(&mut self) -> RadioResult<u16> {
        // Manual p.80: query is "SM;" and the radio responds "SM<4digits>;"
        let raw = self.client.query("SM").await?;
        match ResponseParser::parse(&raw)? {
            Response::SMeter(_sel, reading) => Ok(reading),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected SMeter, got {:?}",
                other
            ))),
        }
    }

    // -----------------------------------------------------------------------
    // PTT
    // -----------------------------------------------------------------------

    /// Assert PTT — switch the radio to transmit.
    pub async fn transmit(&mut self) -> RadioResult<()> {
        self.client.set("TX", "").await
    }

    /// Release PTT — switch the radio to receive.
    pub async fn receive(&mut self) -> RadioResult<()> {
        self.client.set("RX", "").await
    }

    // -----------------------------------------------------------------------
    // Radio identification
    // -----------------------------------------------------------------------

    /// Query the radio's model identifier.
    ///
    /// The TS-570D returns `017`; the TS-570S returns `018` (manual p.73, format 16).
    pub async fn get_id(&mut self) -> RadioResult<u16> {
        let raw = self.client.query("ID").await?;
        match ResponseParser::parse(&raw)? {
            Response::RadioId(id) => Ok(id),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected RadioId, got {:?}",
                other
            ))),
        }
    }

    // -----------------------------------------------------------------------
    // Composite IF information
    // -----------------------------------------------------------------------

    /// Query composite radio status (IF command).
    pub async fn get_information(&mut self) -> RadioResult<InformationResponse> {
        let raw = self.client.query("IF").await?;
        match ResponseParser::parse(&raw)? {
            Response::Information(info) => Ok(info),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected Information, got {:?}",
                other
            ))),
        }
    }

    // -----------------------------------------------------------------------
    // AF gain
    // -----------------------------------------------------------------------

    /// Query the AF (audio) gain level (main receiver).
    ///
    /// The `AG` response carries a 1-digit selector followed by a 3-digit
    /// level.  This method returns only the level for the main receiver
    /// (selector = 0).
    pub async fn get_af_gain(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("AG").await?;
        match ResponseParser::parse(&raw)? {
            Response::AfGain(_sel, level) => Ok(level),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected AfGain, got {:?}",
                other
            ))),
        }
    }

    /// Set the AF (audio) gain level for the main receiver.
    ///
    /// Manual p.75: Set format is `AG<P1:3>;` (3 digits, format 31, 000–255).
    /// No selector digit in the set command.
    pub async fn set_af_gain(&mut self, level: u8) -> RadioResult<()> {
        self.client.set("AG", &format!("{:03}", level)).await
    }

    // -----------------------------------------------------------------------
    // RF gain
    // -----------------------------------------------------------------------

    /// Query the RF gain level.
    pub async fn get_rf_gain(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("RG").await?;
        match ResponseParser::parse(&raw)? {
            Response::RfGain(level) => Ok(level),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected RfGain, got {:?}",
                other
            ))),
        }
    }

    /// Set the RF gain level (0–255).
    pub async fn set_rf_gain(&mut self, level: u8) -> RadioResult<()> {
        self.client.set("RG", &format!("{:03}", level)).await
    }

    // -----------------------------------------------------------------------
    // Transmit power
    // -----------------------------------------------------------------------

    /// Query the transmit power setting (watts, 5–100).
    pub async fn get_power(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("PC").await?;
        match ResponseParser::parse(&raw)? {
            Response::Power(watts) => Ok(watts),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected Power, got {:?}",
                other
            ))),
        }
    }

    /// Set the transmit power (watts, 5–100).
    pub async fn set_power(&mut self, watts: u8) -> RadioResult<()> {
        self.client.set("PC", &format!("{:03}", watts)).await
    }

    // -----------------------------------------------------------------------
    // Noise blanker
    // -----------------------------------------------------------------------

    /// Get noise blanker state.
    pub async fn get_noise_blanker(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("NB").await?;
        match ResponseParser::parse(&raw)? {
            Response::NoiseBlanker(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected NoiseBlanker, got {:?}",
                other
            ))),
        }
    }

    /// Set noise blanker on/off.
    pub async fn set_noise_blanker(&mut self, on: bool) -> RadioResult<()> {
        self.client.set("NB", if on { "1" } else { "0" }).await
    }

    // -----------------------------------------------------------------------
    // Noise reduction
    // -----------------------------------------------------------------------

    /// Get noise reduction level (0=off, 1=NR1, 2=NR2).
    pub async fn get_noise_reduction(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("NR").await?;
        match ResponseParser::parse(&raw)? {
            Response::NoiseReduction(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected NoiseReduction, got {:?}",
                other
            ))),
        }
    }

    /// Set noise reduction level.
    pub async fn set_noise_reduction(&mut self, level: u8) -> RadioResult<()> {
        self.client.set("NR", &format!("{}", level)).await
    }

    // -----------------------------------------------------------------------
    // Pre-amplifier
    // -----------------------------------------------------------------------

    /// Get pre-amplifier state.
    pub async fn get_preamp(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("PA").await?;
        match ResponseParser::parse(&raw)? {
            Response::Preamp(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected Preamp, got {:?}",
                other
            ))),
        }
    }

    /// Set pre-amplifier on/off.
    pub async fn set_preamp(&mut self, on: bool) -> RadioResult<()> {
        self.client.set("PA", if on { "1" } else { "0" }).await
    }

    // -----------------------------------------------------------------------
    // Attenuator
    // -----------------------------------------------------------------------

    /// Get attenuator state.
    pub async fn get_attenuator(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("RA").await?;
        match ResponseParser::parse(&raw)? {
            Response::Attenuator(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected Attenuator, got {:?}",
                other
            ))),
        }
    }

    /// Set attenuator on/off.
    pub async fn set_attenuator(&mut self, on: bool) -> RadioResult<()> {
        self.client.set("RA", if on { "01" } else { "00" }).await
    }

    // -----------------------------------------------------------------------
    // Squelch
    // -----------------------------------------------------------------------

    /// Get squelch level (0–255).
    pub async fn get_squelch(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("SQ").await?;
        match ResponseParser::parse(&raw)? {
            Response::Squelch(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected Squelch, got {:?}",
                other
            ))),
        }
    }

    /// Set squelch level (0–255).
    pub async fn set_squelch(&mut self, level: u8) -> RadioResult<()> {
        self.client.set("SQ", &format!("{:03}", level)).await
    }

    // -----------------------------------------------------------------------
    // Microphone gain
    // -----------------------------------------------------------------------

    /// Get microphone gain (0–100).
    pub async fn get_mic_gain(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("MG").await?;
        match ResponseParser::parse(&raw)? {
            Response::MicGain(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected MicGain, got {:?}",
                other
            ))),
        }
    }

    /// Set microphone gain (0–100).
    pub async fn set_mic_gain(&mut self, gain: u8) -> RadioResult<()> {
        self.client.set("MG", &format!("{:03}", gain)).await
    }

    // -----------------------------------------------------------------------
    // AGC
    // -----------------------------------------------------------------------

    /// Get AGC time constant (2=fast, 4=slow).
    pub async fn get_agc(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("GT").await?;
        match ResponseParser::parse(&raw)? {
            Response::Agc(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected Agc, got {:?}",
                other
            ))),
        }
    }

    /// Set AGC time constant (2=fast, 4=slow).
    pub async fn set_agc(&mut self, constant: u8) -> RadioResult<()> {
        self.client.set("GT", &format!("{:03}", constant)).await
    }

    // -----------------------------------------------------------------------
    // RIT
    // -----------------------------------------------------------------------

    /// Get RIT on/off state.
    pub async fn get_rit(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("RT").await?;
        match ResponseParser::parse(&raw)? {
            Response::Rit(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected Rit, got {:?}",
                other
            ))),
        }
    }

    /// Set RIT on/off.
    pub async fn set_rit(&mut self, on: bool) -> RadioResult<()> {
        self.client.set("RT", if on { "1" } else { "0" }).await
    }

    /// Clear RIT/XIT offset to zero.
    pub async fn clear_rit(&mut self) -> RadioResult<()> {
        self.client.set("RC", "").await
    }

    /// Increment RIT/XIT offset.
    pub async fn rit_up(&mut self) -> RadioResult<()> {
        self.client.set("RU", "").await
    }

    /// Decrement RIT/XIT offset.
    pub async fn rit_down(&mut self) -> RadioResult<()> {
        self.client.set("RD", "").await
    }

    // -----------------------------------------------------------------------
    // XIT
    // -----------------------------------------------------------------------

    /// Get XIT on/off state.
    pub async fn get_xit(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("XT").await?;
        match ResponseParser::parse(&raw)? {
            Response::Xit(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected Xit, got {:?}",
                other
            ))),
        }
    }

    /// Set XIT on/off.
    pub async fn set_xit(&mut self, on: bool) -> RadioResult<()> {
        self.client.set("XT", if on { "1" } else { "0" }).await
    }

    // -----------------------------------------------------------------------
    // Scan
    // -----------------------------------------------------------------------

    /// Get scan on/off state.
    pub async fn get_scan(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("SC").await?;
        match ResponseParser::parse(&raw)? {
            Response::Scan(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected Scan, got {:?}",
                other
            ))),
        }
    }

    /// Set scan on/off.
    pub async fn set_scan(&mut self, on: bool) -> RadioResult<()> {
        self.client.set("SC", if on { "1" } else { "0" }).await
    }

    // -----------------------------------------------------------------------
    // VOX
    // -----------------------------------------------------------------------

    /// Get VOX on/off state.
    pub async fn get_vox(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("VX").await?;
        match ResponseParser::parse(&raw)? {
            Response::Vox(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected Vox, got {:?}",
                other
            ))),
        }
    }

    /// Set VOX on/off.
    pub async fn set_vox(&mut self, on: bool) -> RadioResult<()> {
        self.client.set("VX", if on { "1" } else { "0" }).await
    }

    /// Get VOX gain (1–9).
    pub async fn get_vox_gain(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("VG").await?;
        match ResponseParser::parse(&raw)? {
            Response::VoxGain(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected VoxGain, got {:?}",
                other
            ))),
        }
    }

    /// Set VOX gain (1–9).
    pub async fn set_vox_gain(&mut self, gain: u8) -> RadioResult<()> {
        self.client.set("VG", &format!("{:03}", gain)).await
    }

    /// Get VOX delay in milliseconds (0–3000).
    pub async fn get_vox_delay(&mut self) -> RadioResult<u16> {
        let raw = self.client.query("VD").await?;
        match ResponseParser::parse(&raw)? {
            Response::VoxDelay(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected VoxDelay, got {:?}",
                other
            ))),
        }
    }

    /// Set VOX delay in milliseconds (0–3000).
    pub async fn set_vox_delay(&mut self, ms: u16) -> RadioResult<()> {
        self.client.set("VD", &format!("{:04}", ms)).await
    }

    // -----------------------------------------------------------------------
    // RX/TX VFO selection
    // -----------------------------------------------------------------------

    /// Get receiver VFO/memory selection (0=VFO A, 1=VFO B, 2=Memory).
    pub async fn get_rx_vfo(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("FR").await?;
        match ResponseParser::parse(&raw)? {
            Response::RxVfo(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected RxVfo, got {:?}",
                other
            ))),
        }
    }

    /// Set receiver VFO/memory selection.
    pub async fn set_rx_vfo(&mut self, vfo: u8) -> RadioResult<()> {
        self.client.set("FR", &format!("{}", vfo)).await
    }

    /// Get transmitter VFO/memory selection (0=VFO A, 1=VFO B, 2=Memory).
    pub async fn get_tx_vfo(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("FT").await?;
        match ResponseParser::parse(&raw)? {
            Response::TxVfo(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected TxVfo, got {:?}",
                other
            ))),
        }
    }

    /// Set transmitter VFO/memory selection.
    pub async fn set_tx_vfo(&mut self, vfo: u8) -> RadioResult<()> {
        self.client.set("FT", &format!("{}", vfo)).await
    }

    // -----------------------------------------------------------------------
    // Frequency lock
    // -----------------------------------------------------------------------

    /// Get frequency lock state.
    pub async fn get_frequency_lock(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("LK").await?;
        match ResponseParser::parse(&raw)? {
            Response::FrequencyLock(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected FrequencyLock, got {:?}",
                other
            ))),
        }
    }

    /// Set frequency lock on/off.
    pub async fn set_frequency_lock(&mut self, on: bool) -> RadioResult<()> {
        self.client.set("LK", if on { "1" } else { "0" }).await
    }

    // -----------------------------------------------------------------------
    // Power on/off
    // -----------------------------------------------------------------------

    /// Get transceiver power on/off state.
    pub async fn get_power_on(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("PS").await?;
        match ResponseParser::parse(&raw)? {
            Response::PowerOn(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected PowerOn, got {:?}",
                other
            ))),
        }
    }

    /// Set transceiver power on/off.
    pub async fn set_power_on(&mut self, on: bool) -> RadioResult<()> {
        self.client.set("PS", if on { "1" } else { "0" }).await
    }

    // -----------------------------------------------------------------------
    // Busy status
    // -----------------------------------------------------------------------

    /// Check if receiver is busy (carrier detected).
    pub async fn is_busy(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("BY").await?;
        match ResponseParser::parse(&raw)? {
            Response::Busy(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected Busy, got {:?}",
                other
            ))),
        }
    }

    // -----------------------------------------------------------------------
    // Speech processor
    // -----------------------------------------------------------------------

    /// Get speech processor on/off state.
    pub async fn get_speech_processor(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("PR").await?;
        match ResponseParser::parse(&raw)? {
            Response::SpeechProcessor(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected SpeechProcessor, got {:?}",
                other
            ))),
        }
    }

    /// Set speech processor on/off.
    pub async fn set_speech_processor(&mut self, on: bool) -> RadioResult<()> {
        self.client.set("PR", if on { "1" } else { "0" }).await
    }

    // -----------------------------------------------------------------------
    // Memory channel
    // -----------------------------------------------------------------------

    /// Get current memory channel number (0–99).
    pub async fn get_memory_channel(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("MC").await?;
        match ResponseParser::parse(&raw)? {
            Response::MemoryChannel(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected MemoryChannel, got {:?}",
                other
            ))),
        }
    }

    /// Set memory channel number (0–99).
    pub async fn set_memory_channel(&mut self, ch: u8) -> RadioResult<()> {
        self.client.set("MC", &format!("{:02}", ch)).await
    }

    /// Read memory channel contents (MR command).
    ///
    /// Sends `MR0NN;` where NN is zero-padded 2-digit channel number.
    /// Manual p.78: Read format is `MR<P1><P3>;` with no space (P1=split type,
    /// P3=channel 2 digits).  The answer has a space between P1 and P3.
    /// Parses the radio's `MR...;` response into a [`MemoryChannelEntry`].
    pub async fn read_memory_channel(&mut self, ch: u8) -> RadioResult<MemoryChannelEntry> {
        let raw = self
            .client
            .query_with_param("MR", &format!("0{:02}", ch))
            .await?;
        match ResponseParser::parse(&raw)? {
            Response::MemoryRead(entry) => Ok(entry),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected MemoryRead, got {:?}",
                other
            ))),
        }
    }

    /// Write memory channel contents (MW command).
    ///
    /// Formats and sends `MW<P1> <P3><P4><P5><P6><P7><P8>;`. No response is
    /// expected from the radio.
    pub async fn write_memory_channel(
        &mut self,
        ch: u8,
        entry: MemoryChannelEntry,
    ) -> RadioResult<()> {
        let params = format!(
            "{} {:02}{:011}{}{}{}{:02}",
            if entry.split { 1 } else { 0 },
            ch,
            entry.freq_hz,
            entry.mode,
            if entry.lockout { 1 } else { 0 },
            entry.tone_type,
            entry.tone_number,
        );
        self.client.set("MW", &params).await
    }

    /// Clear a memory channel by writing a vacant entry (MW command).
    ///
    /// Sends `MW0 NN00000000000000000000;`.
    pub async fn clear_memory_channel(&mut self, ch: u8) -> RadioResult<()> {
        let params = format!("0 {:02}00000000000000000000", ch);
        self.client.set("MW", &params).await
    }

    // -----------------------------------------------------------------------
    // Antenna
    // -----------------------------------------------------------------------

    /// Get antenna selection (1=ANT1, 2=ANT2).
    pub async fn get_antenna(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("AN").await?;
        match ResponseParser::parse(&raw)? {
            Response::Antenna(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected Antenna, got {:?}",
                other
            ))),
        }
    }

    /// Set antenna selection (1=ANT1, 2=ANT2).
    pub async fn set_antenna(&mut self, ant: u8) -> RadioResult<()> {
        self.client.set("AN", &format!("{}", ant)).await
    }

    // -----------------------------------------------------------------------
    // CW keyer (inherent — TS-570D-specific)
    // -----------------------------------------------------------------------

    /// Send a CW message via the keyer buffer.
    pub async fn send_cw(&mut self, message: &str) -> RadioResult<()> {
        self.client.set("KY", message).await
    }

    /// Get keyer speed in WPM (10–60).
    pub async fn get_keyer_speed(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("KS").await?;
        match ResponseParser::parse(&raw)? {
            Response::KeyerSpeed(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected KeyerSpeed, got {:?}",
                other
            ))),
        }
    }

    /// Set keyer speed in WPM (10–60).
    pub async fn set_keyer_speed(&mut self, wpm: u8) -> RadioResult<()> {
        self.client.set("KS", &format!("{:03}", wpm)).await
    }

    /// Get CW pitch index (00–12 maps to 400–1000 Hz).
    pub async fn get_cw_pitch(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("PT").await?;
        match ResponseParser::parse(&raw)? {
            Response::CwPitch(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected CwPitch, got {:?}",
                other
            ))),
        }
    }

    /// Set CW pitch index (00–12).
    pub async fn set_cw_pitch(&mut self, pitch: u8) -> RadioResult<()> {
        self.client.set("PT", &format!("{:02}", pitch)).await
    }

    // -----------------------------------------------------------------------
    // Antenna tuner (inherent — TS-570D-specific)
    // -----------------------------------------------------------------------

    /// Set antenna tuner to through (bypass) or tuner mode.
    ///
    /// AC SET format: `AC[P2][P3]` — 2 digits (P2=0:THRU/1:IN, P3=0:off/1:tune).
    pub async fn set_antenna_tuner_thru(&mut self, thru: bool) -> RadioResult<()> {
        self.client.set("AC", if thru { "00" } else { "10" }).await
    }

    /// Start antenna tuning (AC SET with P2=IN, P3=tune-start).
    pub async fn start_antenna_tuning(&mut self) -> RadioResult<()> {
        self.client.set("AC", "11").await
    }

    // -----------------------------------------------------------------------
    // DSP slope filters (inherent — TS-570D-specific)
    // -----------------------------------------------------------------------

    /// Get high cutoff filter index (SH, 00–20).
    pub async fn get_high_cutoff(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("SH").await?;
        match ResponseParser::parse(&raw)? {
            Response::HighCutoff(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected HighCutoff, got {:?}",
                other
            ))),
        }
    }

    /// Set high cutoff filter index (00–20).
    pub async fn set_high_cutoff(&mut self, val: u8) -> RadioResult<()> {
        self.client.set("SH", &format!("{:02}", val)).await
    }

    /// Get low cutoff filter index (SL, 00–20).
    pub async fn get_low_cutoff(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("SL").await?;
        match ResponseParser::parse(&raw)? {
            Response::LowCutoff(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected LowCutoff, got {:?}",
                other
            ))),
        }
    }

    /// Set low cutoff filter index (00–20).
    pub async fn set_low_cutoff(&mut self, val: u8) -> RadioResult<()> {
        self.client.set("SL", &format!("{:02}", val)).await
    }

    // -----------------------------------------------------------------------
    // Tone / CTCSS (inherent — TS-570D-specific)
    // -----------------------------------------------------------------------

    /// Get CTCSS tone number (01–39).
    pub async fn get_ctcss_tone_number(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("CN").await?;
        match ResponseParser::parse(&raw)? {
            Response::CtcssTone(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected CtcssTone, got {:?}",
                other
            ))),
        }
    }

    /// Set CTCSS tone number (01–39).
    pub async fn set_ctcss_tone_number(&mut self, n: u8) -> RadioResult<()> {
        self.client.set("CN", &format!("{:02}", n)).await
    }

    /// Get CTCSS on/off state.
    pub async fn get_ctcss(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("CT").await?;
        match ResponseParser::parse(&raw)? {
            Response::Ctcss(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected Ctcss, got {:?}",
                other
            ))),
        }
    }

    /// Set CTCSS on/off.
    pub async fn set_ctcss(&mut self, on: bool) -> RadioResult<()> {
        self.client.set("CT", if on { "1" } else { "0" }).await
    }

    /// Get tone number (01–39).
    pub async fn get_tone_number(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("TN").await?;
        match ResponseParser::parse(&raw)? {
            Response::ToneNumber(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected ToneNumber, got {:?}",
                other
            ))),
        }
    }

    /// Set tone number (01–39).
    pub async fn set_tone_number(&mut self, n: u8) -> RadioResult<()> {
        self.client.set("TN", &format!("{:02}", n)).await
    }

    /// Get tone on/off state.
    pub async fn get_tone(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("TO").await?;
        match ResponseParser::parse(&raw)? {
            Response::Tone(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected Tone, got {:?}",
                other
            ))),
        }
    }

    /// Set tone on/off.
    pub async fn set_tone(&mut self, on: bool) -> RadioResult<()> {
        self.client.set("TO", if on { "1" } else { "0" }).await
    }

    // -----------------------------------------------------------------------
    // Beat cancel (inherent — TS-570D DSP feature)
    // -----------------------------------------------------------------------

    /// Get beat cancel mode (0=off, 1=on, 2=enhanced).
    pub async fn get_beat_cancel(&mut self) -> RadioResult<u8> {
        let raw = self.client.query("BC").await?;
        match ResponseParser::parse(&raw)? {
            Response::BeatCancel(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected BeatCancel, got {:?}",
                other
            ))),
        }
    }

    /// Set beat cancel mode (0=off, 1=on, 2=enhanced).
    pub async fn set_beat_cancel(&mut self, mode: u8) -> RadioResult<()> {
        self.client.set("BC", &format!("{}", mode)).await
    }

    // -----------------------------------------------------------------------
    // IF shift (inherent)
    // -----------------------------------------------------------------------

    /// Get IF shift direction and frequency.
    pub async fn get_if_shift(&mut self) -> RadioResult<(char, u16)> {
        let raw = self.client.query("IS").await?;
        match ResponseParser::parse(&raw)? {
            Response::IfShift(dir, freq) => Ok((dir, freq)),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected IfShift, got {:?}",
                other
            ))),
        }
    }

    /// Set IF shift direction and frequency.
    pub async fn set_if_shift(&mut self, direction: char, freq: u16) -> RadioResult<()> {
        self.client
            .set("IS", &format!("{}{:04}", direction, freq))
            .await
    }

    // -----------------------------------------------------------------------
    // Voice synthesizer (inherent — TS-570D-specific)
    // -----------------------------------------------------------------------

    /// Recall voice message (1 or 2).
    pub async fn voice_recall(&mut self, voice: u8) -> RadioResult<()> {
        self.client.set("VR", &format!("{}", voice)).await
    }

    // -----------------------------------------------------------------------
    // System reset (inherent — TS-570D-specific)
    // -----------------------------------------------------------------------

    /// Reset the transceiver (1=partial, 2=full).
    pub async fn reset(&mut self, full: bool) -> RadioResult<()> {
        self.client.set("SR", if full { "2" } else { "1" }).await
    }

    // -----------------------------------------------------------------------
    // Meter reading (inherent)
    // -----------------------------------------------------------------------

    /// Read meter value (1=SWR, 2=COMP, 3=ALC).
    pub async fn get_meter(&mut self, meter: u8) -> RadioResult<u16> {
        let raw = self
            .client
            .query_with_param("RM", &format!("{}", meter))
            .await?;
        match ResponseParser::parse(&raw)? {
            Response::Meter(_meter_type, value) => Ok(value),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected Meter, got {:?}",
                other
            ))),
        }
    }

    // -----------------------------------------------------------------------
    // Semi break-in delay (inherent)
    // -----------------------------------------------------------------------

    /// Get semi break-in delay in ms (0–1000, 50ms steps).
    pub async fn get_semi_break_in_delay(&mut self) -> RadioResult<u16> {
        let raw = self.client.query("SD").await?;
        match ResponseParser::parse(&raw)? {
            Response::SemiBreakInDelay(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected SemiBreakInDelay, got {:?}",
                other
            ))),
        }
    }

    /// Set semi break-in delay in ms (0–1000, 50ms steps).
    pub async fn set_semi_break_in_delay(&mut self, ms: u16) -> RadioResult<()> {
        self.client.set("SD", &format!("{:04}", ms)).await
    }

    // -----------------------------------------------------------------------
    // CW auto zero-beat (inherent)
    // -----------------------------------------------------------------------

    /// Get CW auto zero-beat state.
    pub async fn get_cw_auto_zerobeat(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("CA").await?;
        match ResponseParser::parse(&raw)? {
            Response::CwAutoZerobeat(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected CwAutoZerobeat, got {:?}",
                other
            ))),
        }
    }

    /// Set CW auto zero-beat on/off.
    pub async fn set_cw_auto_zerobeat(&mut self, on: bool) -> RadioResult<()> {
        self.client.set("CA", if on { "1" } else { "0" }).await
    }

    // -----------------------------------------------------------------------
    // Fine step (inherent)
    // -----------------------------------------------------------------------

    /// Get fine step state.
    pub async fn get_fine_step(&mut self) -> RadioResult<bool> {
        let raw = self.client.query("FS").await?;
        match ResponseParser::parse(&raw)? {
            Response::FineStep(v) => Ok(v),
            other => Err(RadioError::InvalidProtocolString(format!(
                "expected FineStep, got {:?}",
                other
            ))),
        }
    }

    /// Set fine step on/off.
    pub async fn set_fine_step(&mut self, on: bool) -> RadioResult<()> {
        self.client.set("FS", if on { "1" } else { "0" }).await
    }

    // -----------------------------------------------------------------------
    // Auto information (inherent)
    // -----------------------------------------------------------------------

    /// Set auto information mode (0–3).
    pub async fn set_auto_info(&mut self, mode: u8) -> RadioResult<()> {
        self.client.set("AI", &format!("{}", mode)).await
    }

    // -----------------------------------------------------------------------
    // MIC up/down (inherent — write-only momentary)
    // -----------------------------------------------------------------------

    /// Send MIC Up command.
    pub async fn mic_up(&mut self) -> RadioResult<()> {
        self.client.set("UP", "").await
    }

    /// Send MIC Down command.
    pub async fn mic_down(&mut self) -> RadioResult<()> {
        self.client.set("DN", "").await
    }

    /// Flush the transport receive buffer, discarding unsolicited or stale data.
    pub fn flush_rx(&mut self) {
        self.client.transport.flush_rx();
    }
}

// ---------------------------------------------------------------------------
// Radio trait implementation
// ---------------------------------------------------------------------------

#[async_trait::async_trait(?Send)]
impl<T: framework::transport::Transport> framework::radio::Radio for Ts570d<T> {
    async fn get_vfo_a(&mut self) -> framework::radio::RadioResult<framework::radio::Frequency> {
        Ts570d::get_vfo_a(self).await
    }

    async fn set_vfo_a(
        &mut self,
        freq: framework::radio::Frequency,
    ) -> framework::radio::RadioResult<()> {
        Ts570d::set_vfo_a(self, freq).await
    }

    async fn get_vfo_b(&mut self) -> framework::radio::RadioResult<framework::radio::Frequency> {
        Ts570d::get_vfo_b(self).await
    }

    async fn set_vfo_b(
        &mut self,
        freq: framework::radio::Frequency,
    ) -> framework::radio::RadioResult<()> {
        Ts570d::set_vfo_b(self, freq).await
    }

    async fn get_mode(&mut self) -> framework::radio::RadioResult<framework::radio::Mode> {
        Ts570d::get_mode(self).await
    }

    async fn set_mode(
        &mut self,
        mode: framework::radio::Mode,
    ) -> framework::radio::RadioResult<()> {
        Ts570d::set_mode(self, mode).await
    }

    async fn get_smeter(&mut self) -> framework::radio::RadioResult<u16> {
        Ts570d::get_smeter(self).await
    }

    async fn transmit(&mut self) -> framework::radio::RadioResult<()> {
        Ts570d::transmit(self).await
    }

    async fn receive(&mut self) -> framework::radio::RadioResult<()> {
        Ts570d::receive(self).await
    }

    async fn get_id(&mut self) -> framework::radio::RadioResult<u16> {
        Ts570d::get_id(self).await
    }

    async fn get_information(
        &mut self,
    ) -> framework::radio::RadioResult<framework::radio::InformationResponse> {
        Ts570d::get_information(self).await
    }

    async fn get_af_gain(&mut self) -> framework::radio::RadioResult<u8> {
        Ts570d::get_af_gain(self).await
    }

    async fn set_af_gain(&mut self, level: u8) -> framework::radio::RadioResult<()> {
        Ts570d::set_af_gain(self, level).await
    }

    async fn get_rf_gain(&mut self) -> framework::radio::RadioResult<u8> {
        Ts570d::get_rf_gain(self).await
    }

    async fn set_rf_gain(&mut self, level: u8) -> framework::radio::RadioResult<()> {
        Ts570d::set_rf_gain(self, level).await
    }

    async fn get_power(&mut self) -> framework::radio::RadioResult<u8> {
        Ts570d::get_power(self).await
    }

    async fn set_power(&mut self, watts: u8) -> framework::radio::RadioResult<()> {
        Ts570d::set_power(self, watts).await
    }

    async fn get_noise_blanker(&mut self) -> framework::radio::RadioResult<bool> {
        Ts570d::get_noise_blanker(self).await
    }

    async fn set_noise_blanker(&mut self, on: bool) -> framework::radio::RadioResult<()> {
        Ts570d::set_noise_blanker(self, on).await
    }

    async fn get_noise_reduction(&mut self) -> framework::radio::RadioResult<u8> {
        Ts570d::get_noise_reduction(self).await
    }

    async fn set_noise_reduction(&mut self, level: u8) -> framework::radio::RadioResult<()> {
        Ts570d::set_noise_reduction(self, level).await
    }

    async fn get_preamp(&mut self) -> framework::radio::RadioResult<bool> {
        Ts570d::get_preamp(self).await
    }

    async fn set_preamp(&mut self, on: bool) -> framework::radio::RadioResult<()> {
        Ts570d::set_preamp(self, on).await
    }

    async fn get_attenuator(&mut self) -> framework::radio::RadioResult<bool> {
        Ts570d::get_attenuator(self).await
    }

    async fn set_attenuator(&mut self, on: bool) -> framework::radio::RadioResult<()> {
        Ts570d::set_attenuator(self, on).await
    }

    async fn get_squelch(&mut self) -> framework::radio::RadioResult<u8> {
        Ts570d::get_squelch(self).await
    }

    async fn set_squelch(&mut self, level: u8) -> framework::radio::RadioResult<()> {
        Ts570d::set_squelch(self, level).await
    }

    async fn get_mic_gain(&mut self) -> framework::radio::RadioResult<u8> {
        Ts570d::get_mic_gain(self).await
    }

    async fn set_mic_gain(&mut self, gain: u8) -> framework::radio::RadioResult<()> {
        Ts570d::set_mic_gain(self, gain).await
    }

    async fn get_agc(&mut self) -> framework::radio::RadioResult<u8> {
        Ts570d::get_agc(self).await
    }

    async fn set_agc(&mut self, constant: u8) -> framework::radio::RadioResult<()> {
        Ts570d::set_agc(self, constant).await
    }

    async fn get_rit(&mut self) -> framework::radio::RadioResult<bool> {
        Ts570d::get_rit(self).await
    }

    async fn set_rit(&mut self, on: bool) -> framework::radio::RadioResult<()> {
        Ts570d::set_rit(self, on).await
    }

    async fn clear_rit(&mut self) -> framework::radio::RadioResult<()> {
        Ts570d::clear_rit(self).await
    }

    async fn rit_up(&mut self) -> framework::radio::RadioResult<()> {
        Ts570d::rit_up(self).await
    }

    async fn rit_down(&mut self) -> framework::radio::RadioResult<()> {
        Ts570d::rit_down(self).await
    }

    async fn get_xit(&mut self) -> framework::radio::RadioResult<bool> {
        Ts570d::get_xit(self).await
    }

    async fn set_xit(&mut self, on: bool) -> framework::radio::RadioResult<()> {
        Ts570d::set_xit(self, on).await
    }

    async fn get_scan(&mut self) -> framework::radio::RadioResult<bool> {
        Ts570d::get_scan(self).await
    }

    async fn set_scan(&mut self, on: bool) -> framework::radio::RadioResult<()> {
        Ts570d::set_scan(self, on).await
    }

    async fn get_vox(&mut self) -> framework::radio::RadioResult<bool> {
        Ts570d::get_vox(self).await
    }

    async fn set_vox(&mut self, on: bool) -> framework::radio::RadioResult<()> {
        Ts570d::set_vox(self, on).await
    }

    async fn get_vox_gain(&mut self) -> framework::radio::RadioResult<u8> {
        Ts570d::get_vox_gain(self).await
    }

    async fn set_vox_gain(&mut self, gain: u8) -> framework::radio::RadioResult<()> {
        Ts570d::set_vox_gain(self, gain).await
    }

    async fn get_vox_delay(&mut self) -> framework::radio::RadioResult<u16> {
        Ts570d::get_vox_delay(self).await
    }

    async fn set_vox_delay(&mut self, ms: u16) -> framework::radio::RadioResult<()> {
        Ts570d::set_vox_delay(self, ms).await
    }

    async fn get_rx_vfo(&mut self) -> framework::radio::RadioResult<u8> {
        Ts570d::get_rx_vfo(self).await
    }

    async fn set_rx_vfo(&mut self, vfo: u8) -> framework::radio::RadioResult<()> {
        Ts570d::set_rx_vfo(self, vfo).await
    }

    async fn get_tx_vfo(&mut self) -> framework::radio::RadioResult<u8> {
        Ts570d::get_tx_vfo(self).await
    }

    async fn set_tx_vfo(&mut self, vfo: u8) -> framework::radio::RadioResult<()> {
        Ts570d::set_tx_vfo(self, vfo).await
    }

    async fn get_frequency_lock(&mut self) -> framework::radio::RadioResult<bool> {
        Ts570d::get_frequency_lock(self).await
    }

    async fn set_frequency_lock(&mut self, on: bool) -> framework::radio::RadioResult<()> {
        Ts570d::set_frequency_lock(self, on).await
    }

    async fn get_power_on(&mut self) -> framework::radio::RadioResult<bool> {
        Ts570d::get_power_on(self).await
    }

    async fn set_power_on(&mut self, on: bool) -> framework::radio::RadioResult<()> {
        Ts570d::set_power_on(self, on).await
    }

    async fn is_busy(&mut self) -> framework::radio::RadioResult<bool> {
        Ts570d::is_busy(self).await
    }

    async fn get_speech_processor(&mut self) -> framework::radio::RadioResult<bool> {
        Ts570d::get_speech_processor(self).await
    }

    async fn set_speech_processor(&mut self, on: bool) -> framework::radio::RadioResult<()> {
        Ts570d::set_speech_processor(self, on).await
    }

    async fn get_memory_channel(&mut self) -> framework::radio::RadioResult<u8> {
        Ts570d::get_memory_channel(self).await
    }

    async fn set_memory_channel(&mut self, ch: u8) -> framework::radio::RadioResult<()> {
        Ts570d::set_memory_channel(self, ch).await
    }

    async fn read_memory_channel(
        &mut self,
        ch: u8,
    ) -> framework::radio::RadioResult<framework::radio::MemoryChannelEntry> {
        Ts570d::read_memory_channel(self, ch).await
    }

    async fn write_memory_channel(
        &mut self,
        ch: u8,
        entry: framework::radio::MemoryChannelEntry,
    ) -> framework::radio::RadioResult<()> {
        Ts570d::write_memory_channel(self, ch, entry).await
    }

    async fn clear_memory_channel(&mut self, ch: u8) -> framework::radio::RadioResult<()> {
        Ts570d::clear_memory_channel(self, ch).await
    }

    async fn get_antenna(&mut self) -> framework::radio::RadioResult<u8> {
        Ts570d::get_antenna(self).await
    }

    async fn set_antenna(&mut self, ant: u8) -> framework::radio::RadioResult<()> {
        Ts570d::set_antenna(self, ant).await
    }

    async fn send_cw(&mut self, message: &str) -> framework::radio::RadioResult<()> {
        Ts570d::send_cw(self, message).await
    }

    async fn get_keyer_speed(&mut self) -> framework::radio::RadioResult<u8> {
        Ts570d::get_keyer_speed(self).await
    }

    async fn set_keyer_speed(&mut self, wpm: u8) -> framework::radio::RadioResult<()> {
        Ts570d::set_keyer_speed(self, wpm).await
    }

    async fn get_cw_pitch(&mut self) -> framework::radio::RadioResult<u8> {
        Ts570d::get_cw_pitch(self).await
    }

    async fn set_cw_pitch(&mut self, pitch: u8) -> framework::radio::RadioResult<()> {
        Ts570d::set_cw_pitch(self, pitch).await
    }

    async fn set_antenna_tuner_thru(&mut self, thru: bool) -> framework::radio::RadioResult<()> {
        Ts570d::set_antenna_tuner_thru(self, thru).await
    }

    async fn start_antenna_tuning(&mut self) -> framework::radio::RadioResult<()> {
        Ts570d::start_antenna_tuning(self).await
    }

    async fn get_high_cutoff(&mut self) -> framework::radio::RadioResult<u8> {
        Ts570d::get_high_cutoff(self).await
    }

    async fn set_high_cutoff(&mut self, val: u8) -> framework::radio::RadioResult<()> {
        Ts570d::set_high_cutoff(self, val).await
    }

    async fn get_low_cutoff(&mut self) -> framework::radio::RadioResult<u8> {
        Ts570d::get_low_cutoff(self).await
    }

    async fn set_low_cutoff(&mut self, val: u8) -> framework::radio::RadioResult<()> {
        Ts570d::set_low_cutoff(self, val).await
    }

    async fn get_ctcss_tone_number(&mut self) -> framework::radio::RadioResult<u8> {
        Ts570d::get_ctcss_tone_number(self).await
    }

    async fn set_ctcss_tone_number(&mut self, n: u8) -> framework::radio::RadioResult<()> {
        Ts570d::set_ctcss_tone_number(self, n).await
    }

    async fn get_ctcss(&mut self) -> framework::radio::RadioResult<bool> {
        Ts570d::get_ctcss(self).await
    }

    async fn set_ctcss(&mut self, on: bool) -> framework::radio::RadioResult<()> {
        Ts570d::set_ctcss(self, on).await
    }

    async fn get_tone_number(&mut self) -> framework::radio::RadioResult<u8> {
        Ts570d::get_tone_number(self).await
    }

    async fn set_tone_number(&mut self, n: u8) -> framework::radio::RadioResult<()> {
        Ts570d::set_tone_number(self, n).await
    }

    async fn get_tone(&mut self) -> framework::radio::RadioResult<bool> {
        Ts570d::get_tone(self).await
    }

    async fn set_tone(&mut self, on: bool) -> framework::radio::RadioResult<()> {
        Ts570d::set_tone(self, on).await
    }

    async fn get_beat_cancel(&mut self) -> framework::radio::RadioResult<u8> {
        Ts570d::get_beat_cancel(self).await
    }

    async fn set_beat_cancel(&mut self, mode: u8) -> framework::radio::RadioResult<()> {
        Ts570d::set_beat_cancel(self, mode).await
    }

    async fn get_if_shift(&mut self) -> framework::radio::RadioResult<(char, u16)> {
        Ts570d::get_if_shift(self).await
    }

    async fn set_if_shift(
        &mut self,
        direction: char,
        freq: u16,
    ) -> framework::radio::RadioResult<()> {
        Ts570d::set_if_shift(self, direction, freq).await
    }

    async fn voice_recall(&mut self, voice: u8) -> framework::radio::RadioResult<()> {
        Ts570d::voice_recall(self, voice).await
    }

    async fn reset(&mut self, full: bool) -> framework::radio::RadioResult<()> {
        Ts570d::reset(self, full).await
    }

    async fn get_meter(&mut self, meter: u8) -> framework::radio::RadioResult<u16> {
        Ts570d::get_meter(self, meter).await
    }

    async fn get_semi_break_in_delay(&mut self) -> framework::radio::RadioResult<u16> {
        Ts570d::get_semi_break_in_delay(self).await
    }

    async fn set_semi_break_in_delay(&mut self, ms: u16) -> framework::radio::RadioResult<()> {
        Ts570d::set_semi_break_in_delay(self, ms).await
    }

    async fn get_cw_auto_zerobeat(&mut self) -> framework::radio::RadioResult<bool> {
        Ts570d::get_cw_auto_zerobeat(self).await
    }

    async fn set_cw_auto_zerobeat(&mut self, on: bool) -> framework::radio::RadioResult<()> {
        Ts570d::set_cw_auto_zerobeat(self, on).await
    }

    async fn get_fine_step(&mut self) -> framework::radio::RadioResult<bool> {
        Ts570d::get_fine_step(self).await
    }

    async fn set_fine_step(&mut self, on: bool) -> framework::radio::RadioResult<()> {
        Ts570d::set_fine_step(self, on).await
    }

    async fn set_auto_info(&mut self, mode: u8) -> framework::radio::RadioResult<()> {
        Ts570d::set_auto_info(self, mode).await
    }

    async fn mic_up(&mut self) -> framework::radio::RadioResult<()> {
        Ts570d::mic_up(self).await
    }

    async fn mic_down(&mut self) -> framework::radio::RadioResult<()> {
        Ts570d::mic_down(self).await
    }

    fn flush_rx(&mut self) {
        Ts570d::flush_rx(self);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use framework::errors::TransportError;
    use framework::transport::Transport;
    use std::collections::VecDeque;

    // -----------------------------------------------------------------------
    // In-memory fake transport
    // -----------------------------------------------------------------------

    /// A minimal in-memory transport for unit testing.
    ///
    /// `writes` accumulates every byte written by the client.
    /// `reads` is a queue of bytes the client will read back.
    struct FakeTransport {
        writes: Vec<u8>,
        reads: VecDeque<u8>,
    }

    impl FakeTransport {
        fn new() -> Self {
            Self {
                writes: Vec::new(),
                reads: VecDeque::new(),
            }
        }

        /// Enqueue bytes that `read()` will return to the client.
        fn enqueue_response(&mut self, response: &str) {
            self.reads.extend(response.as_bytes());
        }

        /// Return everything written via `write()`.
        fn written(&self) -> &[u8] {
            &self.writes
        }

        /// Return what was written as a UTF-8 string (panics on non-UTF-8).
        fn written_str(&self) -> &str {
            std::str::from_utf8(&self.writes).expect("non-UTF-8 in writes")
        }
    }

    #[async_trait(?Send)]
    impl Transport for FakeTransport {
        async fn write(&mut self, data: &[u8]) -> Result<usize, TransportError> {
            self.writes.extend_from_slice(data);
            Ok(data.len())
        }

        async fn read(&mut self, buf: &mut [u8]) -> Result<usize, TransportError> {
            if let Some(byte) = self.reads.pop_front() {
                buf[0] = byte;
                Ok(1)
            } else {
                Ok(0)
            }
        }

        async fn flush(&mut self) -> Result<(), TransportError> {
            Ok(())
        }
    }

    // -----------------------------------------------------------------------
    // Helper: build a Ts570d with a pre-configured FakeTransport
    // -----------------------------------------------------------------------

    fn make_radio(response: &str) -> Ts570d<FakeTransport> {
        let mut transport = FakeTransport::new();
        transport.enqueue_response(response);
        Ts570d::new(transport)
    }

    // -----------------------------------------------------------------------
    // get_vfo_a
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_vfo_a_query_sent() {
        let mut radio = make_radio("FA00014250000;");
        let _ = radio.get_vfo_a().await.unwrap();
        assert_eq!(radio.client.transport.written(), b"FA;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_vfo_a_frequency_parsed() {
        let mut radio = make_radio("FA00014250000;");
        let freq = radio.get_vfo_a().await.unwrap();
        assert_eq!(freq, Frequency::new(14_250_000).unwrap());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_vfo_a_frequency_14mhz() {
        let mut radio = make_radio("FA00014100000;");
        let freq = radio.get_vfo_a().await.unwrap();
        assert_eq!(freq.hz(), 14_100_000);
    }

    // -----------------------------------------------------------------------
    // set_vfo_a
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_set_vfo_a_command_formatted() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        let freq = Frequency::new(14_250_000).unwrap();
        radio.set_vfo_a(freq).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "FA00014250000;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_vfo_a_zero_padded() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        let freq = Frequency::new(7_000_000).unwrap();
        radio.set_vfo_a(freq).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "FA00007000000;");
    }

    // -----------------------------------------------------------------------
    // get_vfo_b
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_vfo_b_query_sent() {
        let mut radio = make_radio("FB00007100000;");
        let _ = radio.get_vfo_b().await.unwrap();
        assert_eq!(radio.client.transport.written(), b"FB;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_vfo_b_frequency_parsed() {
        let mut radio = make_radio("FB00007100000;");
        let freq = radio.get_vfo_b().await.unwrap();
        assert_eq!(freq, Frequency::new(7_100_000).unwrap());
    }

    // -----------------------------------------------------------------------
    // set_vfo_b
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_set_vfo_b_command_formatted() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        let freq = Frequency::new(7_100_000).unwrap();
        radio.set_vfo_b(freq).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "FB00007100000;");
    }

    // -----------------------------------------------------------------------
    // get_mode
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_mode_query_sent() {
        let mut radio = make_radio("MD2;");
        let _ = radio.get_mode().await.unwrap();
        assert_eq!(radio.client.transport.written(), b"MD;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_mode_usb_parsed() {
        let mut radio = make_radio("MD2;");
        let mode = radio.get_mode().await.unwrap();
        assert_eq!(mode, Mode::Usb);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_mode_lsb_parsed() {
        let mut radio = make_radio("MD1;");
        let mode = radio.get_mode().await.unwrap();
        assert_eq!(mode, Mode::Lsb);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_mode_cw_parsed() {
        let mut radio = make_radio("MD3;");
        let mode = radio.get_mode().await.unwrap();
        assert_eq!(mode, Mode::Cw);
    }

    // -----------------------------------------------------------------------
    // set_mode
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_set_mode_usb_encoded() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_mode(Mode::Usb).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "MD2;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_mode_lsb_encoded() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_mode(Mode::Lsb).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "MD1;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_mode_cw_reverse_encoded() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_mode(Mode::CwReverse).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "MD7;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_mode_fsk_reverse_encoded() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_mode(Mode::FskReverse).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "MD9;");
    }

    // -----------------------------------------------------------------------
    // get_smeter
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_smeter_query_sent() {
        // Manual p.80: Answer is SM<4digits>; — no selector prefix.
        let mut radio = make_radio("SM0015;");
        let _ = radio.get_smeter().await.unwrap();
        assert_eq!(radio.client.transport.written(), b"SM;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_smeter_value_parsed() {
        // Manual p.80: SM<4digits>; canonical form.
        let mut radio = make_radio("SM0015;");
        let reading = radio.get_smeter().await.unwrap();
        assert_eq!(reading, 15);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_smeter_zero() {
        let mut radio = make_radio("SM0000;");
        let reading = radio.get_smeter().await.unwrap();
        assert_eq!(reading, 0);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_smeter_max() {
        // Maximum S-meter value per manual is 0015 (S9+60 dB).
        let mut radio = make_radio("SM0015;");
        let reading = radio.get_smeter().await.unwrap();
        assert_eq!(reading, 15);
    }

    // -----------------------------------------------------------------------
    // transmit / receive
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_transmit_sends_tx() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.transmit().await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "TX;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_receive_sends_rx() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.receive().await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "RX;");
    }

    // -----------------------------------------------------------------------
    // get_id
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_id_query_sent() {
        let mut radio = make_radio("ID019;");
        let _ = radio.get_id().await.unwrap();
        assert_eq!(radio.client.transport.written(), b"ID;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_id_ts570s_parsed() {
        let mut radio = make_radio("ID019;");
        let id = radio.get_id().await.unwrap();
        assert_eq!(id, 19);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_id_ts570d_parsed() {
        let mut radio = make_radio("ID018;");
        let id = radio.get_id().await.unwrap();
        assert_eq!(id, 18);
    }

    // -----------------------------------------------------------------------
    // get_information
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_information_query_sent() {
        // Build a minimal valid IF string (34-char payload after "IF", before ";")
        // payload = "0001423000001000+00000000102000000" (34 chars)
        let if_str = "IF0001423000001000+00000000102000000;";
        let mut radio = make_radio(if_str);
        let _ = radio.get_information().await;
        assert_eq!(radio.client.transport.written(), b"IF;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_information_frequency_parsed() {
        // 34-char payload after "IF":
        //   [0..11]  "00014230000"  freq = 14_230_000
        //   [11..16] "01000"        step = 1000
        //   [16..21] "+0000"        rit/xit offset = 0
        //   [21]     "0"            rit disabled
        //   [22]     "0"            xit disabled
        //   [23]     "0"            memory bank = 0 (1 char)
        //   [24..26] "01"           memory channel = 1
        //   [26]     "0"            rx (not tx)
        //   [27]     "2"            mode = USB
        //   [28]     "0"            vfo mode
        //   [29]     "0"            scan off
        //   [30]     "0"            split off
        //   [31..33] "00"           ctcss tone = 0
        //   [33]     "0"            tone number = 0
        // Total payload: 11+5+5+1+1+1+2+1+1+1+1+1+2+1 = 34 chars
        let if_str = "IF0001423000001000+00000000102000000;";
        let mut radio = make_radio(if_str);
        let info = radio.get_information().await.unwrap();
        assert_eq!(info.frequency, Frequency::new(14_230_000).unwrap());
        assert_eq!(info.mode, Mode::Usb);
    }

    // -----------------------------------------------------------------------
    // get_af_gain / set_af_gain
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_af_gain_query_sent() {
        // Manual p.75: Answer is AG<3digits>; — no selector prefix.
        let mut radio = make_radio("AG128;");
        let _ = radio.get_af_gain().await.unwrap();
        assert_eq!(radio.client.transport.written(), b"AG;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_af_gain_value_parsed() {
        // Manual p.75: AG<3digits>; canonical form.
        let mut radio = make_radio("AG128;");
        let level = radio.get_af_gain().await.unwrap();
        assert_eq!(level, 128);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_af_gain_formatted() {
        // Manual p.75: Set is AG<P1:3>; — 3-digit level, no selector.
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_af_gain(200).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "AG200;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_af_gain_zero_padded() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_af_gain(5).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "AG005;");
    }

    // -----------------------------------------------------------------------
    // get_rf_gain / set_rf_gain
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_rf_gain_query_sent() {
        let mut radio = make_radio("RG200;");
        let _ = radio.get_rf_gain().await.unwrap();
        assert_eq!(radio.client.transport.written(), b"RG;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_rf_gain_value_parsed() {
        let mut radio = make_radio("RG200;");
        let level = radio.get_rf_gain().await.unwrap();
        assert_eq!(level, 200);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_rf_gain_formatted() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_rf_gain(255).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "RG255;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_rf_gain_zero_padded() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_rf_gain(10).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "RG010;");
    }

    // -----------------------------------------------------------------------
    // get_power / set_power
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_power_query_sent() {
        let mut radio = make_radio("PC100;");
        let _ = radio.get_power().await.unwrap();
        assert_eq!(radio.client.transport.written(), b"PC;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_power_value_parsed() {
        let mut radio = make_radio("PC100;");
        let watts = radio.get_power().await.unwrap();
        assert_eq!(watts, 100);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_power_formatted() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_power(100).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "PC100;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_power_min_zero_padded() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_power(5).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "PC005;");
    }

    // -----------------------------------------------------------------------
    // Error propagation
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_vfo_a_bad_response_returns_error() {
        // Parser will return an error for a response with the wrong code
        let mut radio = make_radio("FB00014250000;");
        let result = radio.get_vfo_a().await;
        // The parse succeeds (it's a valid FB response) but we get VfoBFrequency,
        // which does not match VfoAFrequency — our typed client returns an error.
        assert!(result.is_err());
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_mode_bad_response_returns_error() {
        // MD8 is an invalid mode — parser returns an error
        let mut radio = make_radio("MD8;");
        let result = radio.get_mode().await;
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // get_noise_blanker / set_noise_blanker
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_noise_blanker_on() {
        let mut radio = make_radio("NB1;");
        let v = radio.get_noise_blanker().await.unwrap();
        assert!(v);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_noise_blanker_off() {
        let mut radio = make_radio("NB0;");
        let v = radio.get_noise_blanker().await.unwrap();
        assert!(!v);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_noise_blanker_on() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_noise_blanker(true).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "NB1;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_noise_blanker_off() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_noise_blanker(false).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "NB0;");
    }

    // -----------------------------------------------------------------------
    // get_noise_reduction / set_noise_reduction
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_noise_reduction_nr2() {
        let mut radio = make_radio("NR2;");
        let v = radio.get_noise_reduction().await.unwrap();
        assert_eq!(v, 2);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_noise_reduction() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_noise_reduction(1).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "NR1;");
    }

    // -----------------------------------------------------------------------
    // get_preamp / set_preamp
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_preamp_on() {
        let mut radio = make_radio("PA1;");
        let v = radio.get_preamp().await.unwrap();
        assert!(v);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_preamp_on() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_preamp(true).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "PA1;");
    }

    // -----------------------------------------------------------------------
    // get_attenuator / set_attenuator
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_attenuator_off() {
        let mut radio = make_radio("RA00;");
        let v = radio.get_attenuator().await.unwrap();
        assert!(!v);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_attenuator_on() {
        let mut radio = make_radio("RA01;");
        let v = radio.get_attenuator().await.unwrap();
        assert!(v);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_attenuator_on() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_attenuator(true).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "RA01;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_attenuator_off() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_attenuator(false).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "RA00;");
    }

    // -----------------------------------------------------------------------
    // get_squelch / set_squelch
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_squelch() {
        let mut radio = make_radio("SQ100;");
        let v = radio.get_squelch().await.unwrap();
        assert_eq!(v, 100);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_squelch() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_squelch(50).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "SQ050;");
    }

    // -----------------------------------------------------------------------
    // get_mic_gain / set_mic_gain
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_mic_gain() {
        let mut radio = make_radio("MG050;");
        let v = radio.get_mic_gain().await.unwrap();
        assert_eq!(v, 50);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_mic_gain() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_mic_gain(75).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "MG075;");
    }

    // -----------------------------------------------------------------------
    // get_agc / set_agc
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_agc_fast() {
        let mut radio = make_radio("GT002;");
        let v = radio.get_agc().await.unwrap();
        assert_eq!(v, 2);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_agc() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_agc(4).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "GT004;");
    }

    // -----------------------------------------------------------------------
    // get_rit / set_rit / clear_rit / rit_up / rit_down
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_rit_on() {
        let mut radio = make_radio("RT1;");
        let v = radio.get_rit().await.unwrap();
        assert!(v);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_rit_on() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_rit(true).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "RT1;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_clear_rit() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.clear_rit().await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "RC;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_rit_up() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.rit_up().await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "RU;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_rit_down() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.rit_down().await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "RD;");
    }

    // -----------------------------------------------------------------------
    // get_xit / set_xit
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_xit_on() {
        let mut radio = make_radio("XT1;");
        let v = radio.get_xit().await.unwrap();
        assert!(v);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_xit_on() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_xit(true).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "XT1;");
    }

    // -----------------------------------------------------------------------
    // get_scan / set_scan
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_scan_off() {
        let mut radio = make_radio("SC0;");
        let v = radio.get_scan().await.unwrap();
        assert!(!v);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_scan_on() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_scan(true).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "SC1;");
    }

    // -----------------------------------------------------------------------
    // get_vox / set_vox / get_vox_gain / set_vox_gain / get_vox_delay / set_vox_delay
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_vox_on() {
        let mut radio = make_radio("VX1;");
        let v = radio.get_vox().await.unwrap();
        assert!(v);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_vox_on() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_vox(true).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "VX1;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_vox_gain() {
        let mut radio = make_radio("VG005;");
        let v = radio.get_vox_gain().await.unwrap();
        assert_eq!(v, 5);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_vox_gain() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_vox_gain(7).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "VG007;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_vox_delay() {
        let mut radio = make_radio("VD1500;");
        let v = radio.get_vox_delay().await.unwrap();
        assert_eq!(v, 1500);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_vox_delay() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_vox_delay(2000).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "VD2000;");
    }

    // -----------------------------------------------------------------------
    // get_rx_vfo / set_rx_vfo / get_tx_vfo / set_tx_vfo
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_rx_vfo_a() {
        let mut radio = make_radio("FR0;");
        let v = radio.get_rx_vfo().await.unwrap();
        assert_eq!(v, 0);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_rx_vfo_b() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_rx_vfo(1).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "FR1;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_tx_vfo_a() {
        let mut radio = make_radio("FT0;");
        let v = radio.get_tx_vfo().await.unwrap();
        assert_eq!(v, 0);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_tx_vfo_b() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_tx_vfo(1).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "FT1;");
    }

    // -----------------------------------------------------------------------
    // get_frequency_lock / set_frequency_lock
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_frequency_lock_on() {
        let mut radio = make_radio("LK1;");
        let v = radio.get_frequency_lock().await.unwrap();
        assert!(v);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_frequency_lock_on() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_frequency_lock(true).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "LK1;");
    }

    // -----------------------------------------------------------------------
    // get_power_on / set_power_on
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_power_on_true() {
        let mut radio = make_radio("PS1;");
        let v = radio.get_power_on().await.unwrap();
        assert!(v);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_power_on_off() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_power_on(false).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "PS0;");
    }

    // -----------------------------------------------------------------------
    // is_busy
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_is_busy_true() {
        let mut radio = make_radio("BY1;");
        let v = radio.is_busy().await.unwrap();
        assert!(v);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_is_busy_false() {
        let mut radio = make_radio("BY0;");
        let v = radio.is_busy().await.unwrap();
        assert!(!v);
    }

    // -----------------------------------------------------------------------
    // get_speech_processor / set_speech_processor
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_speech_processor_on() {
        let mut radio = make_radio("PR1;");
        let v = radio.get_speech_processor().await.unwrap();
        assert!(v);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_speech_processor_on() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_speech_processor(true).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "PR1;");
    }

    // -----------------------------------------------------------------------
    // get_memory_channel / set_memory_channel
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_memory_channel() {
        let mut radio = make_radio("MC05;");
        let v = radio.get_memory_channel().await.unwrap();
        assert_eq!(v, 5);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_memory_channel() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_memory_channel(10).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "MC10;");
    }

    // -----------------------------------------------------------------------
    // get_antenna / set_antenna
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_antenna_1() {
        let mut radio = make_radio("AN1;");
        let v = radio.get_antenna().await.unwrap();
        assert_eq!(v, 1);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_antenna_2() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_antenna(2).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "AN2;");
    }

    // -----------------------------------------------------------------------
    // CW keyer inherent methods
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_send_cw() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.send_cw("CQ").await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "KYCQ;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_keyer_speed() {
        let mut radio = make_radio("KS025;");
        let v = radio.get_keyer_speed().await.unwrap();
        assert_eq!(v, 25);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_keyer_speed() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_keyer_speed(30).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "KS030;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_cw_pitch() {
        let mut radio = make_radio("PT06;");
        let v = radio.get_cw_pitch().await.unwrap();
        assert_eq!(v, 6);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_cw_pitch() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_cw_pitch(8).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "PT08;");
    }

    // -----------------------------------------------------------------------
    // Voice / reset
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_voice_recall() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.voice_recall(1).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "VR1;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_reset_partial() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.reset(false).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "SR1;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_reset_full() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.reset(true).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "SR2;");
    }

    // -----------------------------------------------------------------------
    // Semi break-in delay
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_semi_break_in_delay() {
        let mut radio = make_radio("SD0200;");
        let v = radio.get_semi_break_in_delay().await.unwrap();
        assert_eq!(v, 200);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_semi_break_in_delay() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_semi_break_in_delay(500).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "SD0500;");
    }

    // -----------------------------------------------------------------------
    // CW auto zero-beat
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_cw_auto_zerobeat_on() {
        let mut radio = make_radio("CA1;");
        let v = radio.get_cw_auto_zerobeat().await.unwrap();
        assert!(v);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_cw_auto_zerobeat_on() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_cw_auto_zerobeat(true).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "CA1;");
    }

    // -----------------------------------------------------------------------
    // Fine step
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_fine_step_on() {
        let mut radio = make_radio("FS1;");
        let v = radio.get_fine_step().await.unwrap();
        assert!(v);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_fine_step_on() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_fine_step(true).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "FS1;");
    }

    // -----------------------------------------------------------------------
    // Beat cancel
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_beat_cancel_off() {
        let mut radio = make_radio("BC0;");
        let v = radio.get_beat_cancel().await.unwrap();
        assert_eq!(v, 0);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_beat_cancel_enhanced() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_beat_cancel(2).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "BC2;");
    }

    // -----------------------------------------------------------------------
    // IF shift
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_if_shift() {
        let mut radio = make_radio("IS+0500;");
        let (dir, freq) = radio.get_if_shift().await.unwrap();
        assert_eq!(dir, '+');
        assert_eq!(freq, 500);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_if_shift() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_if_shift('-', 300).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "IS-0300;");
    }

    // -----------------------------------------------------------------------
    // MIC up/down
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_mic_up() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.mic_up().await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "UP;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_mic_down() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.mic_down().await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "DN;");
    }

    // -----------------------------------------------------------------------
    // Auto info
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_set_auto_info() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_auto_info(2).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "AI2;");
    }

    // -----------------------------------------------------------------------
    // get_high_cutoff / set_high_cutoff (F08)
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_high_cutoff_query_sent() {
        let mut radio = make_radio("SH05;");
        let _ = radio.get_high_cutoff().await.unwrap();
        assert_eq!(radio.client.transport.written(), b"SH;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_high_cutoff_value_parsed() {
        let mut radio = make_radio("SH05;");
        let v = radio.get_high_cutoff().await.unwrap();
        assert_eq!(v, 5);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_high_cutoff_max() {
        let mut radio = make_radio("SH20;");
        let v = radio.get_high_cutoff().await.unwrap();
        assert_eq!(v, 20);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_high_cutoff() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_high_cutoff(10).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "SH10;");
    }

    // -----------------------------------------------------------------------
    // get_low_cutoff / set_low_cutoff (F08)
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_low_cutoff_query_sent() {
        let mut radio = make_radio("SL03;");
        let _ = radio.get_low_cutoff().await.unwrap();
        assert_eq!(radio.client.transport.written(), b"SL;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_low_cutoff_value_parsed() {
        let mut radio = make_radio("SL03;");
        let v = radio.get_low_cutoff().await.unwrap();
        assert_eq!(v, 3);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_low_cutoff() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_low_cutoff(7).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "SL07;");
    }

    // -----------------------------------------------------------------------
    // get_meter (F01)
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_get_meter_swr_query_sent() {
        let mut radio = make_radio("RM10023;");
        let _ = radio.get_meter(1).await.unwrap();
        // Must send "RM1;" — the meter type selector belongs in the wire bytes before ';'
        assert_eq!(radio.client.transport.written(), b"RM1;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_meter_swr_value_parsed() {
        let mut radio = make_radio("RM10023;");
        let v = radio.get_meter(1).await.unwrap();
        assert_eq!(v, 23);
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_meter_comp_query_sent() {
        let mut radio = make_radio("RM20010;");
        let _ = radio.get_meter(2).await.unwrap();
        assert_eq!(radio.client.transport.written(), b"RM2;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_meter_alc_query_sent() {
        let mut radio = make_radio("RM30005;");
        let _ = radio.get_meter(3).await.unwrap();
        assert_eq!(radio.client.transport.written(), b"RM3;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_get_meter_zero_value() {
        let mut radio = make_radio("RM10000;");
        let v = radio.get_meter(1).await.unwrap();
        assert_eq!(v, 0);
    }
}
