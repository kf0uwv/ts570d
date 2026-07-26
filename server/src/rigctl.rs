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

//! A Hamlib rigctld-compatible TCP listener, for WSJT-X's "Hamlib NET
//! rigctl" rig type.
//!
//! **Not validated against a real WSJT-X instance in this session** (no
//! WSJT-X available in this sandbox). The command subset below (the short
//! single-letter commands `f`/`F`/`m`/`M`/`t`/`T`/`v`, plus `\dump_state`
//! and `\chk_vfo`) is what Hamlib's `netrigctl.c` backend actually issues --
//! this is not the same wire text as interactive `rigctl`'s long-form
//! `get_freq`/`set_freq`/... commands, which a human types at a REPL, not
//! what `netrigctl.c` sends automatically. `\dump_state`'s exact field
//! layout (frequency/mode/vfo range rows, tuning steps, filters, then a
//! fixed tail of capability numbers) was reconstructed from public
//! documentation of Hamlib's `rigctld.c`/`netrigctl.c` protocol, not from a
//! byte-for-byte spec transcription -- **treat this as a first cut to
//! validate/iterate against real WSJT-X**, especially if the initial
//! connection handshake fails (Hamlib's `rig_open()` calls `dump_state` and
//! can abort the whole connection if it can't parse the reply). Mode/range
//! bitmasks are deliberately permissive (`-1`, "any mode") rather than an
//! attempt at Hamlib's exact per-mode bit assignments, specifically to
//! avoid a wrong narrow mask silently rejecting operations WSJT-X needs.
//!
//! Also out of scope, deliberately: split VFO (`s`/`S`) and VFO selection
//! (`V`) are not backed by anything in `radio::Radio` today, so they are
//! not implemented rather than silently faked -- `RPRT -1` (this bridge's
//! one generic failure code -- see [`RPRT_ERR`]) tells the client honestly
//! that the command isn't supported, rather than reporting success for
//! something that didn't happen.
//!
//! Delegates to `radio::Ts570d`'s existing typed methods via
//! [`crate::broker_session::BrokerCatSession`] -- this module never
//! constructs a raw TS-570D wire frame itself.

use std::io;

use monoio::io::{AsyncReadRent, AsyncWriteRentExt};
use monoio::net::{TcpListener, TcpStream};

use cat_server::{BrokerHandle, ClientId};
use cat_transport_core::{CatSession, TransportError};
use radio::{Frequency, Mode, Ts570d};

use crate::broker_session::BrokerCatSession;

/// Accept loop, mirroring `cat_server::tcp::serve`'s shape: binding is the
/// caller's responsibility, one task per accepted connection, runs until
/// `accept()` itself fails.
pub async fn serve(listener: TcpListener, handle: BrokerHandle) -> io::Result<()> {
    let mut next_client_id: u64 = 0;
    loop {
        let (stream, _peer_addr) = listener.accept().await?;
        let client_id = ClientId::from_raw(next_client_id);
        next_client_id = next_client_id.wrapping_add(1);
        let handle = handle.clone();
        monoio::spawn(handle_connection(stream, handle, client_id));
    }
}

async fn handle_connection(mut stream: TcpStream, handle: BrokerHandle, client_id: ClientId) {
    let mut radio = Ts570d::new(BrokerCatSession::new(handle, client_id));
    let mut reader = LineReader::new();

    loop {
        let line = match reader.read_line(&mut stream).await {
            Ok(Some(line)) => line,
            Ok(None) | Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.eq_ignore_ascii_case("q") {
            break;
        }

        let response = dispatch(&mut radio, trimmed).await;
        let (result, _buf) = stream.write_all(response.into_bytes()).await;
        if result.is_err() {
            break;
        }
    }
}

/// A rigctld error report line: `RPRT <code>`. `-1` is used for every
/// failure here (this bridge does not attempt to reproduce Hamlib's
/// specific per-cause negative error codes -- WSJT-X's own error handling
/// only distinguishes zero from non-zero).
const RPRT_OK: &str = "RPRT 0\n";
const RPRT_ERR: &str = "RPRT -1\n";

/// Dispatch one rigctld command line against `radio`, returning the full
/// response text (already newline-terminated). Generic over the same
/// `CatSession<Error = TransportError>` bound `Ts570d`'s own impl blocks
/// use, so this works against a real `BrokerCatSession` in production and
/// a `ScriptedCatSession` in tests without any test-specific branching.
async fn dispatch<S>(radio: &mut Ts570d<S>, line: &str) -> String
where
    S: CatSession<Error = TransportError>,
{
    let mut parts = line.split_whitespace();
    let Some(cmd) = parts.next() else {
        return RPRT_ERR.to_string();
    };
    let args: Vec<&str> = parts.collect();

    match cmd {
        "f" => match radio.get_vfo_a().await {
            Ok(freq) => format!("{}\n", freq.hz()),
            Err(_) => RPRT_ERR.to_string(),
        },
        "F" => {
            let Some(hz) = args.first().and_then(|s| s.parse::<u64>().ok()) else {
                return RPRT_ERR.to_string();
            };
            match Frequency::new(hz) {
                Ok(freq) => match radio.set_vfo_a(freq).await {
                    Ok(()) => RPRT_OK.to_string(),
                    Err(_) => RPRT_ERR.to_string(),
                },
                Err(_) => RPRT_ERR.to_string(),
            }
        }
        "m" => match radio.get_mode().await {
            // Passband is always reported as `0` ("use the rig's current
            // default") rather than a real bandwidth -- see module docs on
            // why filter-width resolution is out of scope for this bridge.
            Ok(mode) => format!("{}\n0\n", hamlib_mode_name(mode)),
            Err(_) => RPRT_ERR.to_string(),
        },
        "M" => {
            let Some(mode_name) = args.first() else {
                return RPRT_ERR.to_string();
            };
            match hamlib_mode_from_name(mode_name) {
                Some(mode) => match radio.set_mode(mode).await {
                    Ok(()) => RPRT_OK.to_string(),
                    Err(_) => RPRT_ERR.to_string(),
                },
                None => RPRT_ERR.to_string(),
            }
        }
        // No `get_tx_state`/`TxState` on `Ts570d` -- read the `tx_rx` field
        // of the `IF` query's `InformationResponse` instead (module docs).
        "t" => match radio.get_information().await {
            Ok(info) if info.tx_rx => "1\n".to_string(),
            Ok(_) => "0\n".to_string(),
            Err(_) => RPRT_ERR.to_string(),
        },
        "T" => {
            let result = match args.first() {
                Some(&"1") => radio.transmit().await,
                Some(&"0") => radio.receive().await,
                _ => return RPRT_ERR.to_string(),
            };
            match result {
                Ok(()) => RPRT_OK.to_string(),
                Err(_) => RPRT_ERR.to_string(),
            }
        }
        // No VFO-B/split concept on `radio::Radio` today -- always report
        // the single VFO this bridge controls (see module docs).
        "v" => "VFOA\n".to_string(),
        "\\chk_vfo" => "0\n".to_string(),
        "\\dump_state" => dump_state(),
        _ => RPRT_ERR.to_string(),
    }
}

/// Map a [`Mode`] to the Hamlib rig-mode name `m`/`M` exchange on the wire.
/// TS-570D's 8 modes map 1:1 onto Hamlib names -- no fallback/best-effort
/// arms are needed (unlike a radio with a C4FM/data-mode equivalent).
fn hamlib_mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Lsb => "LSB",
        Mode::Usb => "USB",
        Mode::Cw => "CW",
        Mode::Fm => "FM",
        Mode::Am => "AM",
        Mode::Fsk => "RTTY",
        Mode::CwReverse => "CWR",
        Mode::FskReverse => "RTTYR",
    }
}

fn hamlib_mode_from_name(name: &str) -> Option<Mode> {
    match name.to_ascii_uppercase().as_str() {
        "LSB" => Some(Mode::Lsb),
        "USB" => Some(Mode::Usb),
        "CW" => Some(Mode::Cw),
        "FM" => Some(Mode::Fm),
        "AM" => Some(Mode::Am),
        "RTTY" => Some(Mode::Fsk),
        "CWR" => Some(Mode::CwReverse),
        "RTTYR" => Some(Mode::FskReverse),
        _ => None,
    }
}

/// The `\dump_state` capability handshake Hamlib's `netrigctl.c` client
/// sends once, right after connecting -- see this module's doc comment for
/// the "not validated against real WSJT-X" caveat. Frequency range drawn
/// from [`Frequency::MIN_HZ`]/[`Frequency::MAX_HZ`] (TS-570D's real
/// documented receive range: 500 kHz-60 MHz), unlike the invented
/// mode/vfo/antenna bitmasks below.
fn dump_state() -> String {
    let mut s = String::new();
    // Protocol marker, rig model (0 = generic/unknown), ITU region (0 =
    // unspecified) -- the three fixed header lines every `dump_state` reply
    // starts with.
    s.push_str("0\n0\n0\n");

    // RX range list: one row of `start end modes low_power high_power vfo
    // ant`, terminated by an all-zero sentinel row. `modes`/`vfo`/`ant` use
    // `-1` (all bits set) rather than an attempt at Hamlib's exact per-mode
    // bit assignments -- deliberately permissive, see module docs.
    s.push_str(&format!(
        "{} {} -1 -1 -1 -1 -1\n0 0 0 0 0 0 0\n",
        Frequency::MIN_HZ,
        Frequency::MAX_HZ
    ));
    // TX range list -- same shape, terminated the same way.
    s.push_str(&format!(
        "{} {} -1 -1 -1 -1 -1\n0 0 0 0 0 0 0\n",
        Frequency::MIN_HZ,
        Frequency::MAX_HZ
    ));
    // Tuning steps: `modes step_size`, terminated by an all-zero row. `10`
    // Hz is a reconstructed placeholder tuning step, not validated against
    // real WSJT-X (module docs).
    s.push_str("-1 10\n0 0\n");
    // Filters: `modes width`, terminated by an all-zero row. `2400` Hz is a
    // conservative, universally-legal SSB bandwidth placeholder, not a
    // per-mode table -- this bridge doesn't resolve real filter widths
    // (module docs).
    s.push_str("-1 2400\n0 0\n");

    // Fixed capability tail: max RIT/XIT/IF-shift (Hz), announces bitmask,
    // preamp levels list (dB, zero-terminated), attenuator levels list (dB,
    // zero-terminated), then four zero (hex) capability bitmasks --
    // `has_get_func`/`has_set_func`/`has_get_level`/`has_set_level`. This
    // bridge does not claim any Hamlib "func"/"level" capabilities beyond
    // plain freq/mode/ptt, so all four are `0`.
    s.push_str("1200\n0\n1200\n0\n0\n0\n0x0\n0x0\n0x0\n0x0\n");
    s
}

/// Buffers partial reads and splits them into `\n`-terminated lines (with
/// an optional trailing `\r` trimmed) -- rigctld's protocol is line-based
/// text, not `cat-transport-tcp`'s length-prefixed binary framing, so
/// neither that crate's frame codec nor `cat-server::tcp`'s accept loop
/// apply here; this is new, minimal line-buffering built directly on
/// monoio's owned-buffer `AsyncReadRentExt::read`.
/// Maximum buffered line length before [`LineReader::read_line`] gives up
/// and reports an error instead of growing `buf` further. The listener
/// binds `0.0.0.0` with no authentication or connection cap (module docs),
/// and real rigctld command lines are always short -- the longest command
/// `dispatch()` handles, plus arguments, is nowhere near even 100 bytes --
/// so 512 bytes is a generous cap that real traffic never approaches, while
/// still bounding per-connection memory growth from a client that never
/// sends a `\n`.
const MAX_LINE_LEN: usize = 512;

struct LineReader {
    buf: Vec<u8>,
}

impl LineReader {
    fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Returns `Ok(Some(line))` (no trailing `\n`/`\r`) for one complete
    /// line, `Ok(None)` on a clean disconnect with no partial line pending,
    /// or `Err` on any I/O failure -- including a line that exceeds
    /// [`MAX_LINE_LEN`] bytes without a `\n` (see [`MAX_LINE_LEN`]'s docs).
    /// A partial line still in the buffer when the peer disconnects is
    /// returned once as a final "line" (mirrors how a real terminal
    /// client's last unterminated command would still be worth attempting)
    /// rather than silently discarded.
    async fn read_line(&mut self, stream: &mut TcpStream) -> io::Result<Option<String>> {
        loop {
            if let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
                let mut line: Vec<u8> = self.buf.drain(..=pos).collect();
                line.pop(); // trailing '\n'
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
            }

            if self.buf.len() > MAX_LINE_LEN {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "rigctl line exceeded maximum length of {MAX_LINE_LEN} bytes without a newline"
                    ),
                ));
            }

            let chunk = vec![0u8; 4096];
            let (result, chunk) = stream.read(chunk).await;
            let n = result?;
            if n == 0 {
                if self.buf.is_empty() {
                    return Ok(None);
                }
                let line = std::mem::take(&mut self.buf);
                return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
            }
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use super::*;
    use cat_transport_core::test_support::ScriptedCatSession;

    fn radio_with(session: ScriptedCatSession) -> Ts570d<ScriptedCatSession> {
        Ts570d::new(session)
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_f_reports_current_frequency() {
        let session =
            ScriptedCatSession::with_script(vec![cat_transport_core::test_support::Exchange::new(
                "FA;",
                "FA00014250000;",
            )]);
        let mut radio = radio_with(session);
        assert_eq!(dispatch(&mut radio, "f").await, "14250000\n");
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_capital_f_sets_frequency() {
        let session =
            ScriptedCatSession::with_script(vec![cat_transport_core::test_support::Exchange::new(
                "FA00014250000;",
                "",
            )]);
        let mut radio = radio_with(session);
        assert_eq!(dispatch(&mut radio, "F 14250000").await, RPRT_OK);
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_capital_f_rejects_non_numeric_argument() {
        let mut radio = radio_with(ScriptedCatSession::new());
        assert_eq!(dispatch(&mut radio, "F not-a-number").await, RPRT_ERR);
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_capital_f_rejects_out_of_range_frequency() {
        // Above Frequency::MAX_HZ (60 MHz) -- Frequency::new() itself
        // rejects it, never reaching the radio.
        let mut radio = radio_with(ScriptedCatSession::new());
        assert_eq!(dispatch(&mut radio, "F 70000000").await, RPRT_ERR);
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_m_reports_mode_and_placeholder_passband() {
        let session =
            ScriptedCatSession::with_script(vec![cat_transport_core::test_support::Exchange::new(
                "MD;", "MD2;",
            )]);
        let mut radio = radio_with(session);
        assert_eq!(dispatch(&mut radio, "m").await, "USB\n0\n");
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_capital_m_sets_mode() {
        let session =
            ScriptedCatSession::with_script(vec![cat_transport_core::test_support::Exchange::new(
                "MD2;", "",
            )]);
        let mut radio = radio_with(session);
        assert_eq!(dispatch(&mut radio, "M USB 0").await, RPRT_OK);
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_capital_m_rejects_unknown_mode_name() {
        let mut radio = radio_with(ScriptedCatSession::new());
        assert_eq!(dispatch(&mut radio, "M BOGUS 0").await, RPRT_ERR);
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_t_reports_ptt_off() {
        // IF response layout, ground-truthed against
        // radio/src/ts570d.rs's own test_get_information_frequency_parsed
        // (34-char payload: freq(11) step(5) rit/xit(5) rit(1) xit(1)
        // bank(1) channel(2) tx_rx(1) mode(1) vfo(1) scan(1) split(1)
        // ctcss(2) tone(1)) -- tx_rx digit at payload offset 26 is '0'.
        let session =
            ScriptedCatSession::with_script(vec![cat_transport_core::test_support::Exchange::new(
                "IF;",
                "IF0001423000001000+00000000102000000;",
            )]);
        let mut radio = radio_with(session);
        assert_eq!(dispatch(&mut radio, "t").await, "0\n");
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_t_reports_ptt_on() {
        // Same layout, tx_rx digit at payload offset 26 changed to '1'.
        let session =
            ScriptedCatSession::with_script(vec![cat_transport_core::test_support::Exchange::new(
                "IF;",
                "IF0001423000001000+00000000112000000;",
            )]);
        let mut radio = radio_with(session);
        assert_eq!(dispatch(&mut radio, "t").await, "1\n");
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_capital_t_one_transmits() {
        let session =
            ScriptedCatSession::with_script(vec![cat_transport_core::test_support::Exchange::new(
                "TX;", "",
            )]);
        let mut radio = radio_with(session);
        assert_eq!(dispatch(&mut radio, "T 1").await, RPRT_OK);
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_capital_t_zero_receives() {
        let session =
            ScriptedCatSession::with_script(vec![cat_transport_core::test_support::Exchange::new(
                "RX;", "",
            )]);
        let mut radio = radio_with(session);
        assert_eq!(dispatch(&mut radio, "T 0").await, RPRT_OK);
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_v_reports_vfo_a() {
        let mut radio = radio_with(ScriptedCatSession::new());
        assert_eq!(dispatch(&mut radio, "v").await, "VFOA\n");
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_chk_vfo_reports_zero() {
        let mut radio = radio_with(ScriptedCatSession::new());
        assert_eq!(dispatch(&mut radio, "\\chk_vfo").await, "0\n");
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_unknown_command_is_rprt_err() {
        let mut radio = radio_with(ScriptedCatSession::new());
        assert_eq!(dispatch(&mut radio, "bogus").await, RPRT_ERR);
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_dump_state_ends_every_list_with_a_zero_sentinel_row() {
        let mut radio = radio_with(ScriptedCatSession::new());
        let state = dispatch(&mut radio, "\\dump_state").await;
        let lines: Vec<&str> = state.lines().collect();
        // 3 header lines + rx range (1 row + 1 sentinel) + tx range (1 + 1)
        // + tuning steps (1 + 1) + filters (1 + 1) + 10 tail lines.
        assert_eq!(lines.len(), 3 + 2 + 2 + 2 + 2 + 10);
        assert_eq!(lines[4], "0 0 0 0 0 0 0");
        assert_eq!(lines[6], "0 0 0 0 0 0 0");
        assert_eq!(lines[8], "0 0");
        assert_eq!(lines[10], "0 0");
    }

    #[test]
    fn hamlib_mode_round_trips_for_every_supported_mode() {
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
            let name = hamlib_mode_name(mode);
            assert_eq!(
                hamlib_mode_from_name(name),
                Some(mode),
                "mode {mode:?} -> {name} did not round-trip"
            );
        }
    }

    fn bind_loopback() -> (TcpListener, SocketAddr) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("TcpListener::bind failed");
        let addr = listener.local_addr().expect("local_addr failed");
        (listener, addr)
    }

    // M2 (code review 2026-07-25): `LineReader::read_line` used to grow
    // `buf` without bound until it saw a `\n`, on a listener that binds
    // `0.0.0.0` with no auth or connection cap. Drives `read_line` against
    // a real loopback `TcpStream` (mirroring `cat_server::tcp`'s own
    // `bind_loopback`/`TcpStream::connect` test pattern -- there's no
    // existing precedent in this crate for testing `LineReader` against
    // anything other than a real `TcpStream`, since `read_line` takes one
    // concretely, not a trait) and feeds it more than `MAX_LINE_LEN` bytes
    // with no `\n`, asserting `read_line` returns `Err` rather than
    // hanging or growing `buf` forever.
    #[monoio::test(driver = "legacy", timer_enabled = true)]
    async fn read_line_rejects_a_line_longer_than_the_maximum_without_a_newline() {
        let (listener, addr) = bind_loopback();

        let server_task = monoio::spawn(async move {
            let (mut stream, _peer_addr) = listener.accept().await.expect("accept failed");
            let mut reader = LineReader::new();
            reader.read_line(&mut stream).await
        });

        let mut client = TcpStream::connect(addr).await.expect("connect failed");
        // One byte over the cap, with no '\n' anywhere in it.
        let oversized = vec![b'x'; MAX_LINE_LEN + 1];
        let (result, _buf) = client.write_all(oversized).await;
        result.expect("write_all failed");

        let outcome = server_task.await;
        let err = outcome.expect_err("a line over MAX_LINE_LEN with no newline must be an Err");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);

        // The connection must still be usable to observe the failure
        // client-side too (not just wedged) -- read whatever's there
        // (nothing, since read_line never writes a reply itself; this just
        // confirms the client-side socket wasn't left in a broken state by
        // the assertions above). Dropping `client` here closes it cleanly.
        drop(client);
    }
}
