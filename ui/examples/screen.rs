//! Print the TUI, as characters, without a terminal.
//!
//! `cargo run -p ui --example screen`
//!
//! The feedback loop this crate was missing. Both consoles drifted from
//! the design they were built from, and the reason is banal: nobody could
//! see them. A ratatui buffer is text, so there is no excuse here.

use ratatui::{backend::TestBackend, Terminal};

fn main() {
    let (w, h) = (120u16, 40u16);
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    let state = ui::RadioDisplay {
        vfo_a_hz: 14_074_000,
        vfo_b_hz: 14_200_000,
        mode: "USB".to_string(),
        smeter: 17,
        connected: true,
        initializing: false,
        ..Default::default()
    };
    terminal.draw(|f| ui::debug_draw(f, &state)).unwrap();

    let buf = terminal.backend().buffer();
    println!("┌{}┐", "─".repeat(w as usize));
    for y in 0..h {
        let row: String = (0..w).map(|x| buf.get(x, y).symbol().to_string()).collect();
        println!("│{row}│");
    }
    println!("└{}┘", "─".repeat(w as usize));
}
