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

use crate::client::RadioClient;
use crate::error::{RadioError, RadioResult};
use crate::protocol::{Frequency, InformationResponse, Mode, Response, ResponseParser};

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
    /// Returns the raw 0–30 reading from the `SM0` response.
    pub async fn get_smeter(&mut self) -> RadioResult<u16> {
        // The SM command takes a 1-digit selector; querying "SM" sends "SM;"
        // and the radio responds "SM0XXXX;".  The parser expects a 5-char
        // params string (selector + 4-digit reading).
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
    /// The TS-570D returns `018`; the TS-570S returns `019`.
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
    /// Params are formatted as `0<level:03>` (selector `0` = main receiver,
    /// followed by the 3-digit level).
    pub async fn set_af_gain(&mut self, level: u8) -> RadioResult<()> {
        self.client.set("AG", &format!("0{:03}", level)).await
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

    #[monoio::test]
    async fn test_get_vfo_a_query_sent() {
        let mut radio = make_radio("FA00014250000;");
        let _ = radio.get_vfo_a().await.unwrap();
        assert_eq!(radio.client.transport.written(), b"FA;");
    }

    #[monoio::test]
    async fn test_get_vfo_a_frequency_parsed() {
        let mut radio = make_radio("FA00014250000;");
        let freq = radio.get_vfo_a().await.unwrap();
        assert_eq!(freq, Frequency::new(14_250_000).unwrap());
    }

    #[monoio::test]
    async fn test_get_vfo_a_frequency_14mhz() {
        let mut radio = make_radio("FA00014100000;");
        let freq = radio.get_vfo_a().await.unwrap();
        assert_eq!(freq.hz(), 14_100_000);
    }

    // -----------------------------------------------------------------------
    // set_vfo_a
    // -----------------------------------------------------------------------

    #[monoio::test]
    async fn test_set_vfo_a_command_formatted() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        let freq = Frequency::new(14_250_000).unwrap();
        radio.set_vfo_a(freq).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "FA00014250000;");
    }

    #[monoio::test]
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

    #[monoio::test]
    async fn test_get_vfo_b_query_sent() {
        let mut radio = make_radio("FB00007100000;");
        let _ = radio.get_vfo_b().await.unwrap();
        assert_eq!(radio.client.transport.written(), b"FB;");
    }

    #[monoio::test]
    async fn test_get_vfo_b_frequency_parsed() {
        let mut radio = make_radio("FB00007100000;");
        let freq = radio.get_vfo_b().await.unwrap();
        assert_eq!(freq, Frequency::new(7_100_000).unwrap());
    }

    // -----------------------------------------------------------------------
    // set_vfo_b
    // -----------------------------------------------------------------------

    #[monoio::test]
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

    #[monoio::test]
    async fn test_get_mode_query_sent() {
        let mut radio = make_radio("MD2;");
        let _ = radio.get_mode().await.unwrap();
        assert_eq!(radio.client.transport.written(), b"MD;");
    }

    #[monoio::test]
    async fn test_get_mode_usb_parsed() {
        let mut radio = make_radio("MD2;");
        let mode = radio.get_mode().await.unwrap();
        assert_eq!(mode, Mode::Usb);
    }

    #[monoio::test]
    async fn test_get_mode_lsb_parsed() {
        let mut radio = make_radio("MD1;");
        let mode = radio.get_mode().await.unwrap();
        assert_eq!(mode, Mode::Lsb);
    }

    #[monoio::test]
    async fn test_get_mode_cw_parsed() {
        let mut radio = make_radio("MD3;");
        let mode = radio.get_mode().await.unwrap();
        assert_eq!(mode, Mode::Cw);
    }

    // -----------------------------------------------------------------------
    // set_mode
    // -----------------------------------------------------------------------

    #[monoio::test]
    async fn test_set_mode_usb_encoded() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_mode(Mode::Usb).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "MD2;");
    }

    #[monoio::test]
    async fn test_set_mode_lsb_encoded() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_mode(Mode::Lsb).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "MD1;");
    }

    #[monoio::test]
    async fn test_set_mode_cw_reverse_encoded() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_mode(Mode::CwReverse).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "MD7;");
    }

    #[monoio::test]
    async fn test_set_mode_fsk_reverse_encoded() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_mode(Mode::FskReverse).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "MD9;");
    }

    // -----------------------------------------------------------------------
    // get_smeter
    // -----------------------------------------------------------------------

    #[monoio::test]
    async fn test_get_smeter_query_sent() {
        let mut radio = make_radio("SM00015;");
        let _ = radio.get_smeter().await.unwrap();
        assert_eq!(radio.client.transport.written(), b"SM;");
    }

    #[monoio::test]
    async fn test_get_smeter_value_parsed() {
        let mut radio = make_radio("SM00015;");
        let reading = radio.get_smeter().await.unwrap();
        assert_eq!(reading, 15);
    }

    #[monoio::test]
    async fn test_get_smeter_zero() {
        let mut radio = make_radio("SM00000;");
        let reading = radio.get_smeter().await.unwrap();
        assert_eq!(reading, 0);
    }

    #[monoio::test]
    async fn test_get_smeter_max() {
        let mut radio = make_radio("SM00030;");
        let reading = radio.get_smeter().await.unwrap();
        assert_eq!(reading, 30);
    }

    // -----------------------------------------------------------------------
    // transmit / receive
    // -----------------------------------------------------------------------

    #[monoio::test]
    async fn test_transmit_sends_tx() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.transmit().await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "TX;");
    }

    #[monoio::test]
    async fn test_receive_sends_rx() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.receive().await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "RX;");
    }

    // -----------------------------------------------------------------------
    // get_id
    // -----------------------------------------------------------------------

    #[monoio::test]
    async fn test_get_id_query_sent() {
        let mut radio = make_radio("ID019;");
        let _ = radio.get_id().await.unwrap();
        assert_eq!(radio.client.transport.written(), b"ID;");
    }

    #[monoio::test]
    async fn test_get_id_ts570s_parsed() {
        let mut radio = make_radio("ID019;");
        let id = radio.get_id().await.unwrap();
        assert_eq!(id, 19);
    }

    #[monoio::test]
    async fn test_get_id_ts570d_parsed() {
        let mut radio = make_radio("ID018;");
        let id = radio.get_id().await.unwrap();
        assert_eq!(id, 18);
    }

    // -----------------------------------------------------------------------
    // get_information
    // -----------------------------------------------------------------------

    #[monoio::test]
    async fn test_get_information_query_sent() {
        // Build a minimal valid IF string (37-char payload after "IF", before ";")
        // payload = "000142300001000+000000000102000000000" (37 chars)
        let if_str = "IF000142300001000+000000000102000000000;";
        let mut radio = make_radio(if_str);
        let _ = radio.get_information().await;
        assert_eq!(radio.client.transport.written(), b"IF;");
    }

    #[monoio::test]
    async fn test_get_information_frequency_parsed() {
        // 37-char payload after "IF":
        //   [0..11]  "00014230000"  freq = 14_230_000
        //   [11..15] "1000"         step = 1000
        //   [15..20] "+0000"        rit/xit offset = 0
        //   [20]     "0"            rit disabled
        //   [21]     "0"            xit disabled
        //   [22..24] "00"           memory bank = 0
        //   [24..26] "01"           memory channel = 1
        //   [26]     "0"            rx (not tx)
        //   [27]     "2"            mode = USB
        //   [28]     "0"            vfo mode
        //   [29]     "0"            scan off
        //   [30]     "0"            split off
        //   [31..33] "00"           ctcss tone = 0
        //   [33..35] "00"           tone number = 0
        //   [35]     "0"            offset indicator
        //   [36]     "0"            reserved
        // Total payload: 11+4+5+1+1+2+2+1+1+1+1+1+2+2+1+1 = 37 chars
        let if_str = "IF000142300001000+000000000102000000000;";
        let mut radio = make_radio(if_str);
        let info = radio.get_information().await.unwrap();
        assert_eq!(info.frequency, Frequency::new(14_230_000).unwrap());
        assert_eq!(info.mode, Mode::Usb);
    }

    // -----------------------------------------------------------------------
    // get_af_gain / set_af_gain
    // -----------------------------------------------------------------------

    #[monoio::test]
    async fn test_get_af_gain_query_sent() {
        let mut radio = make_radio("AG0128;");
        let _ = radio.get_af_gain().await.unwrap();
        assert_eq!(radio.client.transport.written(), b"AG;");
    }

    #[monoio::test]
    async fn test_get_af_gain_value_parsed() {
        let mut radio = make_radio("AG0128;");
        let level = radio.get_af_gain().await.unwrap();
        assert_eq!(level, 128);
    }

    #[monoio::test]
    async fn test_set_af_gain_formatted() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_af_gain(200).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "AG0200;");
    }

    #[monoio::test]
    async fn test_set_af_gain_zero_padded() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_af_gain(5).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "AG0005;");
    }

    // -----------------------------------------------------------------------
    // get_rf_gain / set_rf_gain
    // -----------------------------------------------------------------------

    #[monoio::test]
    async fn test_get_rf_gain_query_sent() {
        let mut radio = make_radio("RG200;");
        let _ = radio.get_rf_gain().await.unwrap();
        assert_eq!(radio.client.transport.written(), b"RG;");
    }

    #[monoio::test]
    async fn test_get_rf_gain_value_parsed() {
        let mut radio = make_radio("RG200;");
        let level = radio.get_rf_gain().await.unwrap();
        assert_eq!(level, 200);
    }

    #[monoio::test]
    async fn test_set_rf_gain_formatted() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_rf_gain(255).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "RG255;");
    }

    #[monoio::test]
    async fn test_set_rf_gain_zero_padded() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_rf_gain(10).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "RG010;");
    }

    // -----------------------------------------------------------------------
    // get_power / set_power
    // -----------------------------------------------------------------------

    #[monoio::test]
    async fn test_get_power_query_sent() {
        let mut radio = make_radio("PC100;");
        let _ = radio.get_power().await.unwrap();
        assert_eq!(radio.client.transport.written(), b"PC;");
    }

    #[monoio::test]
    async fn test_get_power_value_parsed() {
        let mut radio = make_radio("PC100;");
        let watts = radio.get_power().await.unwrap();
        assert_eq!(watts, 100);
    }

    #[monoio::test]
    async fn test_set_power_formatted() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_power(100).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "PC100;");
    }

    #[monoio::test]
    async fn test_set_power_min_zero_padded() {
        let transport = FakeTransport::new();
        let mut radio = Ts570d::new(transport);
        radio.set_power(5).await.unwrap();
        assert_eq!(radio.client.transport.written_str(), "PC005;");
    }

    // -----------------------------------------------------------------------
    // Error propagation
    // -----------------------------------------------------------------------

    #[monoio::test]
    async fn test_get_vfo_a_bad_response_returns_error() {
        // Parser will return an error for a response with the wrong code
        let mut radio = make_radio("FB00014250000;");
        let result = radio.get_vfo_a().await;
        // The parse succeeds (it's a valid FB response) but we get VfoBFrequency,
        // which does not match VfoAFrequency — our typed client returns an error.
        assert!(result.is_err());
    }

    #[monoio::test]
    async fn test_get_mode_bad_response_returns_error() {
        // MD8 is an invalid mode — parser returns an error
        let mut radio = make_radio("MD8;");
        let result = radio.get_mode().await;
        assert!(result.is_err());
    }
}
