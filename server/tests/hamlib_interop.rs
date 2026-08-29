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

//! What a real Hamlib client receives from **this** radio's bridge.
//!
//! `cat-rigctl` already proves, against a live client, that a capability
//! set can be turned into a `\dump_state` reply Hamlib accepts. What it
//! cannot prove is that *this* capability set can: its fixture publishes
//! one tuning step, and the TS-570D publishes six.
//!
//! That difference is exactly the shape of the bug radio-cat-rs ADR 0005
//! records — a `\dump_state` reply Hamlib disagrees with about length
//! makes `netrigctl_open()` block forever rather than fail, and nothing in
//! the symptom points at the cause. Every unit test passed while that was
//! happening. So the check that matters is not what the string looks like;
//! it is whether a real client gets through the handshake.
//!
//! Linux-only: `server::run` is an `async fn` on monoio here (ADR 0006).

#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};

use cat_transport_core::test_support::{Exchange, ScriptedCatSession};
use server::ServerConfig;

/// Whether a real Hamlib client is available to test against.
///
/// Mirrors `cat-rigctl`'s rule, and for the reason recorded there: where
/// Hamlib was installed on purpose, a missing binary is a failure, not a
/// skip. The signal is `EXPECT_HAMLIB` and deliberately not `CI` — `CI` is
/// set on the Windows runner too, where this file does not even compile.
fn have_rigctl() -> bool {
    if std::process::Command::new("rigctl")
        .arg("--version")
        .output()
        .is_ok()
    {
        return true;
    }
    assert!(
        std::env::var_os("EXPECT_HAMLIB").is_none(),
        "EXPECT_HAMLIB is set but rigctl is not installed. Install libhamlib-utils."
    );
    eprintln!("SKIPPED: rigctl not installed (install libhamlib-utils).");
    false
}

/// A free TCP port, released immediately.
///
/// Racy in principle; the alternative is teaching `ServerConfig` to report
/// the port it bound, which is a production API change for a test's
/// convenience.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .unwrap()
        .port()
}

/// Bring up this crate's real server on a rigctl port, backed by a scripted
/// radio. Returns once the listener is accepting.
fn serve(script: Vec<Exchange>) -> u16 {
    let port = free_port();
    std::thread::spawn(move || {
        // `enable_timer` is not optional here: `cat-rigctl`'s accept loop
        // uses monoio timers, and a runtime built without one panics
        // inside the driver rather than returning an error. The binary
        // says `#[monoio::main(timer_enabled = true)]` for the same
        // reason.
        let mut rt = monoio::RuntimeBuilder::<monoio::LegacyDriver>::new()
            .enable_timer()
            .build()
            .expect("monoio runtime");
        rt.block_on(async move {
            let _ = server::run(
                ScriptedCatSession::with_unordered_script(script),
                ServerConfig {
                    rigctl_port: Some(port),
                    ..Default::default()
                },
            )
            .await;
        });
    });
    // Poll rather than sleep a fixed amount: a fixed sleep is either flaky
    // or slow, and usually both on a loaded machine.
    for _ in 0..200 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return port;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("server did not start listening on {port}");
}

#[test]
fn a_real_hamlib_client_completes_the_handshake_with_this_radios_capabilities() {
    if !have_rigctl() {
        return;
    }
    // `netrigctl_open()` reads `\dump_state` before it will answer
    // anything at all, so getting a frequency back is proof the handshake
    // completed. If the tail were malformed this would hang rather than
    // fail -- which is why the test asserts on a value, not on an exit
    // code.
    // Hamlib asks more than once -- `netrigctl_open()` reads the current
    // frequency as part of opening, before the `f` on the command line
    // ever runs. The script is generous rather than exact because the
    // number of probes is Hamlib's business, not this test's.
    let port = serve(
        std::iter::repeat_with(|| Exchange::new("FA;", "FA00014074000;"))
            .take(16)
            .collect(),
    );
    // Wrapped in `timeout` deliberately: a malformed capability tail makes
    // Hamlib block rather than fail, so an unwrapped call turns this test
    // from "red" into "never finishes" -- which is how the original bug
    // hid, and is not an improvement.
    let out = std::process::Command::new("timeout")
        .args([
            "20",
            "rigctl",
            "-m",
            "2",
            "-r",
            &format!("127.0.0.1:{port}"),
            "f",
        ])
        .output()
        .expect("run rigctl");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("14074000"),
        "Hamlib did not complete the handshake against the TS-570D's \
         capability set; got {stdout:?} / {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn the_client_is_told_this_radios_real_tuning_steps_and_rit_range() {
    if !have_rigctl() {
        return;
    }
    // Before this radio published its capabilities the bridge sent every
    // client the same invented story: a single 10 Hz tuning step and a
    // 1200 Hz RIT limit. Both are wrong, and both are things a client is
    // entitled to believe.
    let port = serve(Vec::new());
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream.write_all(b"\\dump_state\n").expect("write");
    stream.flush().unwrap();

    // Read until the server stops talking. A fixed line count either stops
    // short of the tail or blocks forever waiting for a line that is not
    // coming -- the connection stays open after the reply, so EOF never
    // arrives. The read timeout is what terminates this.
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .expect("set read timeout");
    let mut reply = String::new();
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => reply.push_str(&line),
        }
    }
    assert!(!reply.is_empty(), "no reply to `\\dump_state` at all");

    for step in ["-1 10\n", "-1 100\n", "-1 1000\n", "-1 9000\n"] {
        assert!(
            reply.contains(step),
            "tuning step {step:?} missing from {reply:?}"
        );
    }
    assert!(
        reply.contains("9999\n"),
        "real RIT limit missing from {reply:?}"
    );
    assert!(
        !reply.contains("1200\n"),
        "still sending the placeholder RIT limit: {reply:?}"
    );
    // The six trailing capability bitmasks Hamlib counts before it will
    // return. Short by one and `netrigctl_open()` blocks forever.
    assert_eq!(
        reply.matches("0x0\n").count(),
        6,
        "capability tail is not six lines: {reply:?}"
    );
}
