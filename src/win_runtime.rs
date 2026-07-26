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

//! Windows entry point support: a minimal, single-future, thread-parking
//! executor, replacing `#[monoio::main]` (which cannot exist on Windows —
//! `monoio` requires io_uring, a Linux kernel interface).
//!
//! This drives `run_app()`'s top-level future (argument parsing, opening the
//! chosen transport, and calling `ui::run`) to completion. `ui::run` itself
//! has its own internal two-future scheduler for Windows
//! (`ui::win_sched::block_on_two`, see
//! `docs/adr/0006-windows-concurrency-model.md`) — from this executor's
//! point of view, awaiting `ui::run(radio)` is just one ordinary `Future`
//! that happens to do its own blocking work internally before resolving.
//! The same shape also drives `run_server_mode()`'s single top-level future
//! in server mode.
//!
//! Same ~20-line shape as `radio-cat-rs`'s `cat_server::block_on` (private to
//! that crate, so not reusable directly from here) and as
//! `docs/adr/0004-windows-serial-backend.md` §1 originally sketched for this
//! exact purpose.

use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

struct ThreadWaker(std::thread::Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// Poll `fut` to completion on the calling thread, parking it between
/// `Pending` polls. Correct for any `Future` built on
/// `cat_transport_core::completion` (or any other `Waker`-contract-correct
/// primitive), including one woken from a different OS thread.
pub(crate) fn block_on<F: Future>(fut: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut fut = Box::pin(fut);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::park(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_an_immediately_ready_future() {
        assert_eq!(block_on(async { 1 + 1 }), 2);
    }

    #[test]
    fn drives_a_future_woken_from_a_different_thread() {
        let (tx, rx) = cat_transport_core::completion::channel::<u32>();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(30));
            tx.send(99);
        });

        assert_eq!(block_on(rx), Ok(99));
    }
}
