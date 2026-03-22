use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Gauge, Paragraph},
    Frame,
};

use crate::RadioDisplay;

/// Format a frequency in Hz as "M.KKK.HHH MHz".
fn format_hz(hz: u64) -> String {
    let mhz = hz / 1_000_000;
    let khz = (hz % 1_000_000) / 1_000;
    let hz_rem = hz % 1_000;
    format!("{}.{:03}.{:03} MHz", mhz, khz, hz_rem)
}

/// Convert a raw TS-570D S-meter value (0–30) to a display label.
fn smeter_label(smeter: u16) -> &'static str {
    match smeter {
        0..=2 => "S0",
        3..=4 => "S1",
        5..=6 => "S2",
        7..=8 => "S3",
        9..=10 => "S4",
        11..=12 => "S5",
        13..=14 => "S6",
        15..=16 => "S7",
        17..=18 => "S8",
        19..=20 => "S9",
        21..=24 => "S9+10",
        25..=28 => "S9+20",
        _ => "S9+30",
    }
}

pub fn draw_ui(f: &mut Frame, state: &RadioDisplay) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(f.size());

    // VFO/frequency panel
    let vfo_text = format!(
        "VFO A: {}  Mode: {}\nVFO B: {}",
        format_hz(state.vfo_a_hz),
        state.mode,
        format_hz(state.vfo_b_hz),
    );
    f.render_widget(
        Paragraph::new(vfo_text).block(Block::default().title("Frequency").borders(Borders::ALL)),
        chunks[0],
    );

    // S-meter panel
    let ratio = (state.smeter as f64) / 30.0;
    let label = smeter_label(state.smeter);
    f.render_widget(
        Gauge::default()
            .block(Block::default().title("S-Meter").borders(Borders::ALL))
            .gauge_style(Style::default().fg(Color::Green))
            .ratio(ratio)
            .label(label),
        chunks[1],
    );

    // Status bar
    let tx_indicator = if state.tx { "TX" } else { "RX" };
    let status_text = format!("TS-570D Radio Control  |  {}  |  q: quit", tx_indicator);
    f.render_widget(
        Paragraph::new(status_text).block(Block::default().title("Status").borders(Borders::ALL)),
        chunks[2],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_hz_14mhz() {
        assert_eq!(format_hz(14_000_000), "14.000.000 MHz");
    }

    #[test]
    fn test_format_hz_7250khz() {
        assert_eq!(format_hz(7_250_000), "7.250.000 MHz");
    }

    #[test]
    fn test_smeter_label_s5() {
        assert_eq!(smeter_label(11), "S5");
    }

    #[test]
    fn test_smeter_label_s9plus() {
        assert_eq!(smeter_label(21), "S9+10");
    }

    #[test]
    fn test_radio_display_default() {
        let d = RadioDisplay::default();
        assert_eq!(d.vfo_a_hz, 14_000_000);
    }
}
