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

//! `--rfc2217` under the runtime the application actually runs on.
//!
//! # Why this file exists
//!
//! `cat-transport-rfc2217`'s reader thread wakes the task suspended in
//! `Transport::read` from a **different OS thread**. That is sound against
//! `std::task::Waker`'s documented contract, and it is exactly what
//! `cat_transport_core::completion` was built for — but monoio's waker
//! panics on a cross-thread wake unless its `sync` feature is enabled:
//!
//! ```text
//! thread 'rfc2217-reader' panicked at monoio/src/task/harness.rs:197:17:
//! waker can only be sent across threads when `sync` feature enabled
//! ```
//!
//! **No test in either repository could catch this.** The transport's own
//! loopback tests drive it with `futures::executor::block_on`, whose waker
//! *is* thread-safe, and so do the emulator's. `cat-transport-tcp`'s
//! Windows backend uses the same primitive and never trips it because there
//! is no monoio on Windows. The only thing that hits it is this
//! application, under `#[monoio::main]`, reading a CAT response — and it
//! was found by running the TUI, not by running the suite.
//!
//! So the guard lives here, in the crate whose `Cargo.toml` carries the
//! feature, and it runs on the runtime that needs it. Remove
//! `features = ["sync"]` from the workspace `monoio` dependency and these
//! tests fail — see the next section for why they have to be told to.
//!
//! # Why every read here has a timeout
//!
//! Without the feature, the reader thread's panic unwinds *that thread*
//! only — the suspended task is simply never woken, so the failure mode is
//! a **hang**, not a red test. A guard that hangs is a guard somebody
//! eventually kills and ignores. Each read is therefore bounded, so a
//! missing feature comes out as a named assertion failure in seconds.
//!
//! Linux-gated for the same reason `tests/integration.rs` is: it needs
//! `#[monoio::test]` and the emulator's own Unix-only pieces.

#![cfg(target_os = "linux")]

use std::sync::Arc;
use std::time::Duration;

use cat_transport_rfc2217::{Rfc2217Config, Rfc2217Port};
use cat_transport_serial::SerialCatSession;
use emulator::com;
use emulator::emulator::new_shared_radio;

/// Long enough that a loaded CI machine is not the thing under test; short
/// enough that a missing `sync` feature fails in seconds rather than
/// hanging the suite.
const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// Stand up a virtual radio's COM port and return its address.
fn serve() -> String {
    let radio = new_shared_radio();
    let acc2 = com::new_shared_acc2();
    com::serve(Arc::clone(&radio), Arc::clone(&acc2), "127.0.0.1:0")
        .expect("serve the radio's COM port")
        .to_string()
}

#[monoio::test(timer_enabled = true)]
async fn a_cat_read_completes_under_monoio_rather_than_panicking() {
    // The whole test is the `.await`. Reaching the assertion at all means
    // the reader thread woke this task from another thread and monoio
    // accepted it; without `features = ["sync"]` the reader thread dies
    // and this task is never woken at all.
    let addr = serve();
    let port = Rfc2217Port::connect(addr.as_str(), Rfc2217Config::default())
        .expect("connect to the radio's COM port");
    let mut radio = radio::Ts570d::new(SerialCatSession::new(port));

    let freq = monoio::time::timeout(READ_TIMEOUT, radio.get_vfo_a())
        .await
        .expect(
            "the read never completed -- the reader thread almost certainly \
             panicked waking this task across threads. Is `sync` still in \
             the workspace `monoio` features?",
        )
        .expect("read VFO A");
    assert_eq!(freq.hz(), 14_000_000);
}

#[monoio::test(timer_enabled = true)]
async fn several_reads_in_a_row_keep_working() {
    // One read could conceivably complete without ever suspending, if the
    // response happened to be buffered before the first poll. Several,
    // interleaved with commands, cannot all dodge the wake path.
    let addr = serve();
    let port = Rfc2217Port::connect(addr.as_str(), Rfc2217Config::default())
        .expect("connect to the radio's COM port");
    let mut radio = radio::Ts570d::new(SerialCatSession::new(port));

    for hz in [14_074_000u64, 7_074_000, 21_074_000] {
        radio
            .set_vfo_a(radio::Frequency::new(hz).expect("a frequency this radio covers"))
            .await
            .expect("set VFO A");
        let read_back = monoio::time::timeout(READ_TIMEOUT, radio.get_vfo_a())
            .await
            .expect("a read stopped completing partway through the sequence")
            .expect("read VFO A");
        assert_eq!(read_back.hz(), hz);
    }
}
