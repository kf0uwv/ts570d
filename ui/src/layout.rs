use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::control::{group_command_labels, ControlState};
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

/// Build an inline S-meter bargraph string (20 chars wide).
fn smeter_bar(smeter: u16) -> String {
    let filled = ((smeter as usize).min(30) * 20 / 30).min(20);
    let empty = 20 - filled;
    let mut s = String::with_capacity(22);
    s.push('▐');
    for _ in 0..filled {
        s.push('█');
    }
    for _ in 0..empty {
        s.push('░');
    }
    s.push('▌');
    s
}

/// Compact inline bargraph, `width` chars wide, fill 0.0–1.0.
fn mini_bar(ratio: f64, width: usize) -> String {
    let filled = ((ratio.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    let empty = width - filled;
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

/// Build AGC label from numeric code.
fn agc_label(agc: u8) -> &'static str {
    match agc {
        0 => "Off",
        1 => "Slow",
        2 => "Mid",
        3 => "Fast",
        _ => "?",
    }
}

/// Build noise reduction label.
fn nr_label(nr: u8) -> &'static str {
    match nr {
        1 => "NR1",
        2 => "NR2",
        _ => "OFF",
    }
}

/// Build beat cancel label.
fn bc_label(bc: u8) -> &'static str {
    match bc {
        1 => "BC1",
        2 => "BC2",
        _ => "OFF",
    }
}

/// Style for an ON indicator.
fn on_style() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

/// Style for an OFF indicator.
fn off_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

// ---------------------------------------------------------------------------
// Top-level layout splitter
// ---------------------------------------------------------------------------

/// Split the full terminal area into (header, status, controls) areas.
pub fn split_areas(area: Rect) -> (Rect, Rect, Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Length(7), // Status
            Constraint::Min(4),    // Controls
        ])
        .split(area);
    (chunks[0], chunks[1], chunks[2])
}

// ---------------------------------------------------------------------------
// draw_header
// ---------------------------------------------------------------------------

/// Draw the TS-570D title header block.
pub fn draw_header(f: &mut Frame, area: Rect) {
    let block = Block::default().borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let title = Paragraph::new("TS-570D RADIO CONTROL")
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center);
    f.render_widget(title, inner);
}

// ---------------------------------------------------------------------------
// draw_ui — status panel (accepts explicit area)
// ---------------------------------------------------------------------------

pub fn draw_ui(f: &mut Frame, area: Rect, state: &RadioDisplay) {
    // Outer block with title
    let outer_block = Block::default().title(" Status ").borders(Borders::ALL);
    let inner = outer_block.inner(area);
    f.render_widget(outer_block, area);

    // Inner vertical layout: 5 rows
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Row 1: Primary (VFO + S-meter)
            Constraint::Length(1), // Row 2: Gains
            Constraint::Length(1), // Row 3: Receiver features
            Constraint::Length(1), // Row 4: Flags
            Constraint::Length(1), // Row 5: Status bar
            Constraint::Min(0),    // Filler
        ])
        .split(inner);

    // -----------------------------------------------------------------------
    // Row 1 — Primary
    // -----------------------------------------------------------------------

    let bar = smeter_bar(state.smeter);
    let label = smeter_label(state.smeter);
    let (tx_text, tx_color) = if state.tx {
        ("TX", Color::Red)
    } else {
        ("RX", Color::Green)
    };

    let line1 = Line::from(vec![
        Span::styled("VFO A  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format_hz(state.vfo_a_hz),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("{:<6}", &state.mode),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw("  S "),
        Span::styled(bar, Style::default().fg(Color::Green)),
        Span::raw(" "),
        Span::styled(format!("{:<6}", label), Style::default().fg(Color::Green)),
        Span::styled(
            tx_text,
            Style::default().fg(tx_color).add_modifier(Modifier::BOLD),
        ),
    ]);

    let mut line2_spans = vec![
        Span::styled("VFO B  ", Style::default().fg(Color::DarkGray)),
        Span::styled(format_hz(state.vfo_b_hz), Style::default().fg(Color::White)),
        Span::raw("  "),
    ];

    if state.rit {
        line2_spans.push(Span::styled(
            format!("RIT: {:+}Hz  ", state.rit_xit_offset_hz),
            Style::default().fg(Color::Yellow),
        ));
    }
    if state.xit {
        line2_spans.push(Span::styled(
            format!("XIT: {:+}Hz  ", state.rit_xit_offset_hz),
            Style::default().fg(Color::Yellow),
        ));
    }
    if !state.rit && !state.xit {
        line2_spans.push(Span::styled("RIT:OFF  XIT:OFF  ", off_style()));
    }

    let split_style = if state.split { on_style() } else { off_style() };
    let split_text = if state.split {
        "Split:ON  "
    } else {
        "Split:OFF  "
    };
    line2_spans.push(Span::styled(split_text, split_style));

    if state.memory_mode {
        line2_spans.push(Span::styled(
            format!("CH: {:02}", state.memory_channel),
            Style::default().fg(Color::Cyan),
        ));
    } else {
        line2_spans.push(Span::styled(
            format!("ANT:{}", state.antenna),
            Style::default().fg(Color::DarkGray),
        ));
    }

    let primary = Paragraph::new(vec![line1, Line::from(line2_spans)]);
    f.render_widget(primary, rows[0]);

    // -----------------------------------------------------------------------
    // Row 2 — Gains
    // -----------------------------------------------------------------------

    let label_style = Style::default().fg(Color::DarkGray);
    let value_style = Style::default().fg(Color::White);
    let bracket_style = Style::default().fg(Color::DarkGray);
    let filled_style = Style::default().fg(Color::Yellow);
    let empty_style = Style::default().fg(Color::DarkGray);

    let af_bar = mini_bar(state.af_gain as f64 / 255.0, 10);
    let rf_bar = mini_bar(state.rf_gain as f64 / 255.0, 10);
    let mic_bar = mini_bar(state.mic_gain as f64 / 100.0, 10);

    let af_filled: String = af_bar.chars().filter(|&c| c == '█').collect();
    let af_empty: String = af_bar.chars().filter(|&c| c == '░').collect();
    let rf_filled: String = rf_bar.chars().filter(|&c| c == '█').collect();
    let rf_empty: String = rf_bar.chars().filter(|&c| c == '░').collect();
    let mic_filled: String = mic_bar.chars().filter(|&c| c == '█').collect();
    let mic_empty: String = mic_bar.chars().filter(|&c| c == '░').collect();

    let gains_line = Line::from(vec![
        Span::styled("AF:", label_style),
        Span::styled("[", bracket_style),
        Span::styled(af_filled, filled_style),
        Span::styled(af_empty, empty_style),
        Span::styled("]", bracket_style),
        Span::raw("  "),
        Span::styled("RF:", label_style),
        Span::styled("[", bracket_style),
        Span::styled(rf_filled, filled_style),
        Span::styled(rf_empty, empty_style),
        Span::styled("]", bracket_style),
        Span::raw("  "),
        Span::styled("MIC:", label_style),
        Span::styled("[", bracket_style),
        Span::styled(mic_filled, filled_style),
        Span::styled(mic_empty, empty_style),
        Span::styled("]", bracket_style),
        Span::raw("  "),
        Span::styled("SQL:", label_style),
        Span::styled(format!("{:>3}", state.squelch), value_style),
        Span::raw("  "),
        Span::styled("PWR:", label_style),
        Span::styled(format!("{:3}W", state.power_pct), value_style),
        Span::raw("  "),
        Span::styled("AGC:", label_style),
        Span::styled(agc_label(state.agc), value_style),
    ]);

    f.render_widget(Paragraph::new(gains_line), rows[1]);

    // -----------------------------------------------------------------------
    // Row 3 — Receiver features
    // -----------------------------------------------------------------------

    let nb_style = if state.noise_blanker {
        on_style()
    } else {
        off_style()
    };
    let nb_text = if state.noise_blanker { "ON " } else { "OFF" };

    let nr_text = nr_label(state.noise_reduction);
    let nr_style = if state.noise_reduction != 0 {
        on_style()
    } else {
        off_style()
    };

    let att_style = if state.attenuator {
        on_style()
    } else {
        off_style()
    };
    let att_text = if state.attenuator { "ON " } else { "OFF" };

    let pre_style = if state.preamp {
        on_style()
    } else {
        off_style()
    };
    let pre_text = if state.preamp { "ON " } else { "OFF" };

    let proc_style = if state.speech_processor {
        on_style()
    } else {
        off_style()
    };
    let proc_text = if state.speech_processor { "ON " } else { "OFF" };

    let vox_style = if state.vox { on_style() } else { off_style() };
    let vox_text = if state.vox { "ON " } else { "OFF" };

    let bc_text = bc_label(state.beat_cancel);
    let bc_style = if state.beat_cancel != 0 {
        on_style()
    } else {
        off_style()
    };

    let rx_line = Line::from(vec![
        Span::styled("NB:", label_style),
        Span::styled(nb_text, nb_style),
        Span::raw("  "),
        Span::styled("NR:", label_style),
        Span::styled(nr_text, nr_style),
        Span::raw("  "),
        Span::styled("ATT:", label_style),
        Span::styled(att_text, att_style),
        Span::raw("  "),
        Span::styled("PRE:", label_style),
        Span::styled(pre_text, pre_style),
        Span::raw("  "),
        Span::styled("PROC:", label_style),
        Span::styled(proc_text, proc_style),
        Span::raw("  "),
        Span::styled("VOX:", label_style),
        Span::styled(vox_text, vox_style),
        Span::raw("  "),
        Span::styled("BC:", label_style),
        Span::styled(bc_text, bc_style),
    ]);

    f.render_widget(Paragraph::new(rx_line), rows[2]);

    // -----------------------------------------------------------------------
    // Row 4 — Flags
    // -----------------------------------------------------------------------

    let scan_style = if state.scan { on_style() } else { off_style() };
    let scan_text = if state.scan { "ON " } else { "OFF" };

    let lock_style = if state.freq_lock {
        on_style()
    } else {
        off_style()
    };
    let lock_text = if state.freq_lock { "ON " } else { "OFF" };

    let fine_style = if state.fine_step {
        on_style()
    } else {
        off_style()
    };
    let fine_text = if state.fine_step { "ON " } else { "OFF" };

    let ctcss_style = if state.ctcss { on_style() } else { off_style() };
    let ctcss_text = if state.ctcss { "ON " } else { "OFF" };

    let flags_line = Line::from(vec![
        Span::styled("Scan:", label_style),
        Span::styled(scan_text, scan_style),
        Span::raw("  "),
        Span::styled("Lock:", label_style),
        Span::styled(lock_text, lock_style),
        Span::raw("  "),
        Span::styled("Fine:", label_style),
        Span::styled(fine_text, fine_style),
        Span::raw("  "),
        Span::styled("CTCSS:", label_style),
        Span::styled(ctcss_text, ctcss_style),
    ]);

    f.render_widget(Paragraph::new(flags_line), rows[3]);

    // -----------------------------------------------------------------------
    // Row 5 — Status bar
    // -----------------------------------------------------------------------

    let status_line = Line::from(vec![
        Span::styled(
            "TS-570D Radio Control",
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw("  |  "),
        Span::styled("q: quit", Style::default().fg(Color::DarkGray)),
    ]);

    f.render_widget(Paragraph::new(status_line), rows[4]);
}

// ---------------------------------------------------------------------------
// draw_control_panel
// ---------------------------------------------------------------------------

/// Draw the interactive control panel.
pub fn draw_control_panel(f: &mut Frame, area: Rect, state: &ControlState) {
    let outer_block = Block::default().title(" Controls ").borders(Borders::ALL);
    let inner = outer_block.inner(area);
    f.render_widget(outer_block, area);

    // Split inner area into 3 lines
    let lines = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Line 1: hints / prompt
            Constraint::Length(1), // Line 2: error / blank
            Constraint::Min(1),    // Line 3: input / cursor
        ])
        .split(inner);

    match state {
        ControlState::Menu => {
            let hint = Line::from(vec![
                Span::styled(
                    "[F]",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Frequency  "),
                Span::styled(
                    "[M]",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Mode  "),
                Span::styled(
                    "[A]",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Audio  "),
                Span::styled(
                    "[T]",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Transmit  "),
                Span::styled("[q]", Style::default().fg(Color::DarkGray)),
                Span::raw(" Quit"),
            ]);
            f.render_widget(Paragraph::new(hint), lines[0]);
            // Line 2: blank
            // Line 3: prompt
            f.render_widget(Paragraph::new(">"), lines[2]);
        }

        ControlState::GroupMenu { group, .. } => {
            let labels = group_command_labels(*group);
            let mut spans: Vec<Span> = Vec::new();
            for (i, lbl) in labels.iter().enumerate() {
                spans.push(Span::styled(
                    format!("[{}]", i + 1),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::raw(format!(" {}  ", lbl)));
            }
            spans.push(Span::styled("[Esc]", Style::default().fg(Color::DarkGray)));
            spans.push(Span::raw(" Back"));
            f.render_widget(Paragraph::new(Line::from(spans)), lines[0]);
            f.render_widget(Paragraph::new(">"), lines[2]);
        }

        ControlState::TextInput {
            prompt,
            buffer,
            error,
            ..
        } => {
            f.render_widget(Paragraph::new(prompt.as_str()), lines[0]);
            if let Some(err) = error {
                let err_line = Line::from(vec![Span::styled(
                    format!("⚠ {}", err),
                    Style::default().fg(Color::Red),
                )]);
                f.render_widget(Paragraph::new(err_line), lines[1]);
            }
            let input_line = Line::from(vec![
                Span::raw("> "),
                Span::raw(buffer.as_str()),
                Span::styled("_", Style::default().fg(Color::Yellow)),
            ]);
            f.render_widget(Paragraph::new(input_line), lines[2]);
        }

        ControlState::ListSelect {
            options, cursor, ..
        } => {
            let hint = Line::from("← → to select, Enter to confirm, Esc to cancel");
            f.render_widget(Paragraph::new(hint), lines[0]);

            let mut option_spans: Vec<Span> = vec![Span::raw("> ")];
            for (i, opt) in options.iter().enumerate() {
                if i == *cursor {
                    option_spans.push(Span::styled(
                        format!("[{}]", opt),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ));
                } else {
                    option_spans.push(Span::raw(format!(" {} ", opt)));
                }
                if i + 1 < options.len() {
                    option_spans.push(Span::raw("  "));
                }
            }
            f.render_widget(Paragraph::new(Line::from(option_spans)), lines[2]);
        }

        ControlState::Feedback { message, is_error } => {
            let msg_style = if *is_error {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::Green)
            };
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(message.as_str(), msg_style))),
                lines[1],
            );
            f.render_widget(Paragraph::new("Press any key to continue"), lines[2]);
        }
    }
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
