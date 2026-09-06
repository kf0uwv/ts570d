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

//! Is this argument a device on this machine, or something on the network?
//!
//! Every endpoint this program takes — the CAT port, the IF output, the
//! audio pair — can be either, and the operator should not have to say
//! which with a second flag. The shape of the argument already says it.
//!
//! Shared by `#[path]` rather than through a library, because this package
//! is two binaries and no lib target. That is the ordinary Rust answer for
//! a handful of lines two bins both need, and it keeps the rule in one file
//! where a test can pin it.
//!
//! # How a device is named is the driver's business, not ours
//!
//! There is no single "device path" that spans these, and pretending there
//! is would be the mistake:
//!
//! | endpoint | Linux | Windows |
//! |---|---|---|
//! | serial | `/dev/ttyUSB0` | `COM3` |
//! | sound card | `hw:1,0`, `plughw:CARD=Codec,DEV=0` | a WASAPI device *name* |
//! | RTL-SDR | **not a path at all** | **not a path at all** |
//!
//! The last row is the one that surprises people, and it answers "how do
//! you even specify this on Windows": you do not, on either OS. An RTL-SDR
//! is claimed over USB by `libusb`, not by the kernel's tty or sound
//! layers, so it has no filesystem name anywhere. `librtlsdr` addresses it
//! by **index or serial**, which is why [`RTL_SCHEME`] exists and why the
//! same syntax is correct on Linux and Windows alike.
//!
//! Sound devices are the opposite case: their names are per-host, differ
//! between machines, and are not guessable — which is the argument for
//! enumerating them and letting the operator pick, rather than expecting
//! anyone to type `plughw:CARD=Codec,DEV=0` from memory.

/// How a local RTL-SDR is named: `rtl:<index>`.
///
/// Checked before the `host:port` test, because `rtl:0` would otherwise
/// read as a host called `rtl` on port 0.
pub const RTL_SCHEME: &str = "rtl:";

/// How a local sound card is named: `audio:<name>`.
///
/// The same value `cat_signal_audio::DEVICE_SPEC_PREFIX` carries, and
/// `tests/endpoint_grammar.rs` asserts the two agree. A device called
/// `audio:1234` is not a host on port 1234, and a card name can be
/// anything the driver says it is.
pub const AUDIO_SCHEME: &str = "audio:";

/// Where something lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// `host:port` — reachable over the network.
    Network(String),
    /// A device on this machine, named however its own driver names it.
    Device(String),
}

/// Classify an endpoint argument.
///
/// The order matters. An explicit device scheme wins over the `host:port`
/// shape, because `rtl:0` and `hw:1,0` both end in something that looks
/// like a port number and neither is one.
pub fn classify(target: &str) -> Endpoint {
    if target.starts_with(RTL_SCHEME) || target.starts_with(AUDIO_SCHEME) {
        return Endpoint::Device(target.to_string());
    }
    if is_network(target) {
        Endpoint::Network(target.to_string())
    } else {
        Endpoint::Device(target.to_string())
    }
}

/// Whether `target` names a network endpoint rather than a local device.
///
/// The test is "ends in `:<port>`" with a **non-zero** port. Port zero is
/// excluded deliberately: it means "any free port" to something binding a
/// socket and is never a thing to connect *to*, so treating `hw:1,0` — an
/// ALSA device — as a network address would be wrong in the one case the
/// exclusion costs nothing to avoid.
///
/// Deliberately **not** `to_socket_addrs()`. That resolves DNS, so a typo
/// in a device path would hang on a lookup before anything reported that
/// the device is missing — a confusing failure for the commoner mistake.
pub fn is_network(target: &str) -> bool {
    match target.rsplit_once(':') {
        Some((host, port)) => {
            !host.is_empty() && port.parse::<u16>().map(|p| p != 0).unwrap_or(false)
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(s: &str) -> Endpoint {
        Endpoint::Device(s.to_string())
    }
    fn net(s: &str) -> Endpoint {
        Endpoint::Network(s.to_string())
    }

    #[test]
    fn a_host_and_port_is_a_network_endpoint() {
        assert_eq!(classify("127.0.0.1:4001"), net("127.0.0.1:4001"));
        assert_eq!(classify("radio.local:4001"), net("radio.local:4001"));
        assert_eq!(classify("[::1]:4001"), net("[::1]:4001"));
        assert_eq!(classify("localhost:1"), net("localhost:1"));
    }

    #[test]
    fn a_serial_device_path_is_a_device_on_both_platforms() {
        assert_eq!(classify("/dev/ttyUSB0"), dev("/dev/ttyUSB0"));
        assert_eq!(classify("/dev/pts/5"), dev("/dev/pts/5"));
        assert_eq!(classify("COM3"), dev("COM3"));
        assert_eq!(
            classify("/dev/serial/by-id/usb-FTDI_FT232R-if00-port0"),
            dev("/dev/serial/by-id/usb-FTDI_FT232R-if00-port0")
        );
    }

    #[test]
    fn an_alsa_device_is_not_mistaken_for_a_host_on_port_zero() {
        // `hw:1,0` ends in `:1,0`, and the last colon leaves `0` behind.
        // Without the non-zero rule this would be opened as a socket.
        assert_eq!(classify("hw:1,0"), dev("hw:1,0"));
        assert_eq!(
            classify("plughw:CARD=Codec,DEV=0"),
            dev("plughw:CARD=Codec,DEV=0")
        );
        assert_eq!(classify("default"), dev("default"));
    }

    #[test]
    fn an_sdr_is_addressed_by_index_or_serial_and_never_by_path() {
        // The whole reason for an explicit scheme: `rtl:0` is a host called
        // `rtl` on port 0 by shape, and is not one. An RTL-SDR has no
        // filesystem name on Linux or Windows -- libusb claims it, so
        // librtlsdr addresses it by index or serial and the same syntax is
        // right on both.
        assert_eq!(classify("rtl:0"), dev("rtl:0"));
        assert_eq!(classify("rtl:1"), dev("rtl:1"));
        assert_eq!(classify("rtl:00000001"), dev("rtl:00000001"));
    }

    #[test]
    fn a_sound_card_is_named_by_the_driver_and_may_look_like_anything() {
        // Card names are whatever the driver says. One that happens to
        // parse as a port must still be a device.
        assert_eq!(classify("audio:default"), dev("audio:default"));
        assert_eq!(classify("audio:HDA Intel"), dev("audio:HDA Intel"));
        assert_eq!(classify("audio:1234"), dev("audio:1234"));
    }

    #[test]
    fn a_colon_with_no_usable_port_behind_it_is_a_device() {
        assert_eq!(classify("/dev/weird:name"), dev("/dev/weird:name"));
        assert_eq!(classify(":4001"), dev(":4001"));
        assert_eq!(classify("host:"), dev("host:"));
        assert_eq!(classify("host:notanumber"), dev("host:notanumber"));
        // 65536 does not fit a u16, so it is not a port.
        assert_eq!(classify("host:65536"), dev("host:65536"));
        // Port zero is "any free port" when binding and meaningless when
        // connecting.
        assert_eq!(classify("host:0"), dev("host:0"));
    }

    #[test]
    fn is_network_agrees_with_classify() {
        // Two callers use the predicate directly; they must not be able to
        // disagree with the classifier about the same string.
        for s in [
            "127.0.0.1:4001",
            "/dev/ttyUSB0",
            "COM3",
            "hw:1,0",
            "rtl:0",
            "audio:default",
            "host:0",
        ] {
            let by_predicate = is_network(s);
            let by_classify = matches!(classify(s), Endpoint::Network(_));
            // `rtl:` is the one deliberate divergence: the scheme wins, and
            // no caller passes an SDR spec to the serial flags.
            if !s.starts_with(RTL_SCHEME) && !s.starts_with(AUDIO_SCHEME) {
                assert_eq!(by_predicate, by_classify, "{s}");
            }
        }
    }
}
