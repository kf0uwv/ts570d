//! Print the TUI, as characters, without a terminal.
//!
//! `cargo run -p ui --example screen`
//!
//! With live signal sources attached — the emulator's, or a real station's:
//!
//! ```sh
//! CN4=127.0.0.1:1234 ACC2=127.0.0.1:4002 cargo run -p ui --example screen
//! ```
//!
//! Without them the console draws its absent states, which are as much a
//! part of the design as the populated ones and are the harder half to get
//! right.
//!
//! The feedback loop this crate was missing. Both consoles drifted from
//! the design they were built from, and the reason is banal: nobody could
//! see them. A ratatui buffer is text, so there is no excuse here.

use ratatui::{backend::TestBackend, Terminal};

fn main() {
    let (w, h) = (120u16, 40u16);
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    let mut state = ui::RadioDisplay {
        vfo_a_hz: 14_074_000,
        vfo_b_hz: 14_200_000,
        mode: "USB".to_string(),
        smeter: 17,
        connected: true,
        initializing: false,
        ..Default::default()
    };
    // Attach whatever the environment offers, and give the sources a
    // moment to produce something. A still taken before the first frame
    // would show the pending state and say nothing about the live one.
    let mut view = ui::console::ConsoleView::for_capabilities(&cat_native::CapabilitiesWire::from(
        &radio::capabilities::TS570D,
    ));
    let mut sources = ui::feeds::ConsoleSources::default();
    if let Ok(addr) = std::env::var("CN4") {
        let iq = cat_signal_rtlsdr::RtlTcpSource::connect(addr.as_str()).expect("CN4 tap");
        let source = cat_signal_rtlsdr::RtlSdrSource::new(
            iq,
            96_000,
            2048,
            cat_signal::IfTapConfig {
                if_center_hz: 73_050_000,
                inverted: true,
                trim_hz: 0,
            },
        );
        sources.spectrum = Some(ui::feeds::SpectrumFeed::start(source, state.vfo_a_hz));
    }
    if let Ok(addr) = std::env::var("ACC2") {
        let (stream, _tx) = cat_signal_audio::AudioStream::connect(
            addr.as_str(),
            cat_signal_audio::AudioPipelineConfig::default(),
        )
        .expect("ACC2 audio");
        sources.audio = Some(ui::feeds::AudioFeed::new(stream));
    }
    if sources.spectrum.is_some() || sources.audio.is_some() {
        std::thread::sleep(std::time::Duration::from_millis(1500));
    }
    if let Some(feed) = sources.spectrum.as_ref() {
        view.spectrum = feed.frames();
    }
    if let Some(audio) = sources.audio.as_mut() {
        audio.poll();
        view.audio = audio.state();
        if let Some(frame) = audio.latest() {
            view.af_scope = Some(frame.scope.clone());
            view.af_spectrum = Some(frame.spectrum.clone());
        }
    }
    // A still with no mode id would lose the AF passband marks, which is
    // exactly the sort of silent difference a still exists to catch.
    state.mode_id = crate_mode_id(&state.mode);
    let caps = cat_native::CapabilitiesWire::from(&radio::capabilities::TS570D);
    view.passband = state
        .mode_id
        .and_then(|mode| cat_ui::af::passband_for(&caps, mode));

    terminal
        .draw(|f| ui::debug_draw_view(f, &state, &view))
        .unwrap();

    let buf = terminal.backend().buffer();
    println!("┌{}┐", "─".repeat(w as usize));
    for y in 0..h {
        let row: String = (0..w).map(|x| buf.get(x, y).symbol().to_string()).collect();
        println!("│{row}│");
    }
    println!("└{}┘", "─".repeat(w as usize));
}

/// The mode id for a label this example set by hand.
///
/// Only the still renderer needs this: the running console gets the id
/// from the radio and never parses a label.
fn crate_mode_id(label: &str) -> Option<cat_native::ModeId> {
    [
        radio::Mode::Lsb,
        radio::Mode::Usb,
        radio::Mode::Cw,
        radio::Mode::Fm,
        radio::Mode::Am,
        radio::Mode::Fsk,
        radio::Mode::CwReverse,
        radio::Mode::FskReverse,
    ]
    .into_iter()
    .find(|m| m.name() == label)
    .map(radio::capabilities::from_mode)
}
