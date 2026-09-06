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

//! TS-570D Terminal UI
//!
//! Provides the ratatui-based terminal interface for the radio controller.

// The console lives in `cat-ui-ratatui` now. It was 1805 lines whose only
// radio-specific parts were a meter table and a mode-label lookup, both of
// which the capability document already answers. Re-exported so this
// crate's own paths are unchanged.
pub use cat_ui_ratatui::console;
pub(crate) mod control;
pub(crate) mod diag;
pub mod feeds;
pub(crate) mod layout;
mod terminal;
// Not `#[cfg(target_os = "windows")]`-gated (see its own module doc): it has
// no actual Windows-specific code and gets real test coverage on every
// platform. Its only production caller is `terminal::run`'s Windows
// variant, so it is legitimately unused in a non-Windows production build —
// `allow(dead_code)` there only, not on Windows (where it is used).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod win_sched;

pub use terminal::run;
// The PTT-line-carrying variant of `run`. The wiring layer picks between
// them by whether the transport it opened has modem control lines at all:
// `--port` and `--rfc2217` do, `--server` does not. See `docs/adr/0010`.
/// [`run_with_ptt_line`], for a console with a spectrum and/or audio
/// source attached. See `feeds`.
pub use terminal::run_console;
pub use terminal::run_with_ptt_line;

/// Draw the whole console into a frame, for `examples/screen.rs`.
///
/// Exists so the layout can be looked at without a terminal. Not part of
/// the running application's path — `terminal::run` owns that — but it
/// draws the same panels through the same functions, so what it shows is
/// what an operator sees.
pub fn debug_draw(f: &mut ratatui::Frame, state: &RadioDisplay) {
    let caps = cat_native::CapabilitiesWire::from(&radio::capabilities::TS570D);
    let view = console::ConsoleView::for_capabilities(&caps);
    console::draw(f, f.size(), state, &view, &caps);
}

/// Draw the console with a caller-supplied view, for a still of a state the
/// default does not reach (a tab that is not the first, a command line
/// mid-type, a spectrum that has frames in it).
pub fn debug_draw_view(f: &mut ratatui::Frame, state: &RadioDisplay, view: &console::ConsoleView) {
    let caps = cat_native::CapabilitiesWire::from(&radio::capabilities::TS570D);
    console::draw(f, f.size(), state, view, &caps);
}

#[derive(Debug, thiserror::Error)]
pub enum UiError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type UiResult<T> = Result<T, UiError>;

// `RadioDisplay` moved to `cat-ui` when a second radio needed the same
// console. It was already radio-generic in shape -- a dial, a mode,
// meters, gains -- and two copies would have agreed until one was edited.
pub use cat_ui::display::RadioDisplay;
