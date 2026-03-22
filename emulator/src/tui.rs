use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::radio_state::{RadioState, VfoSel};

/// Bright amber / yellow style used for ON annunciators.
fn on_style() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

/// Dim style used for OFF annunciators.
fn off_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

/// Render a single annunciator label.
fn annunciator<'a>(label: &'a str, active: bool) -> Span<'a> {
    Span::styled(
        label,
        if active { on_style() } else { off_style() },
    )
}

/// Separator between annunciators.
fn sep() -> Span<'static> {
    Span::styled(" ", Style::default())
}

/// Format the active frequency from state.
fn format_freq(state: &RadioState) -> String {
    let freq_hz = match state.active_vfo {
        VfoSel::B => state.vfo_b_hz,
        _ => state.vfo_a_hz,
    };
    let mhz = freq_hz / 1_000_000;
    let khz = (freq_hz % 1_000_000) / 1_000;
    let ten_hz = (freq_hz % 1_000) / 10;
    format!("{}.{:03}.{:02}", mhz, khz, ten_hz)
}

/// Build the bargraph string from a 0.0–1.0 fill ratio and a total character width.
fn bargraph(ratio: f64, width: usize) -> String {
    // Block elements from lowest to highest fill.
    const BLOCKS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if width == 0 {
        return String::new();
    }
    let total_eighths = (ratio.clamp(0.0, 1.0) * (width * 8) as f64).round() as usize;
    let full_blocks = total_eighths / 8;
    let remainder = total_eighths % 8;
    let mut s = String::with_capacity(width);
    for _ in 0..full_blocks.min(width) {
        s.push('█');
    }
    if full_blocks < width && remainder > 0 {
        s.push(BLOCKS[remainder - 1]);
        // Pad the rest with spaces.
        for _ in (full_blocks + 1)..width {
            s.push(' ');
        }
    } else {
        for _ in full_blocks..width {
            s.push(' ');
        }
    }
    s
}

/// Draw the full TS-570D LCD-style display into `f`.
pub fn draw(f: &mut Frame, state: &RadioState) {
    let area = f.size();

    // Outer border styled amber/yellow.
    let outer_block = Block::default()
        .borders(Borders::ALL)
        .title("TS-570D")
        .border_style(Style::default().fg(Color::Yellow));
    let inner = outer_block.inner(area);
    f.render_widget(outer_block, area);

    // Divide inner area into 5 rows.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Row 1: top annunciators
            Constraint::Length(1), // Row 2: VFO + frequency
            Constraint::Length(1), // Row 3: meter bargraph
            Constraint::Length(1), // Row 4: mode + bottom annunciators
            Constraint::Length(1), // Row 5: aux 2-digit display
        ])
        .split(inner);

    // ------------------------------------------------------------------ Row 1
    // Top-row annunciators.
    let row1_spans = vec![
        annunciator("TX", state.tx),
        sep(),
        annunciator("RX", !state.tx),
        sep(),
        annunciator("AT", state.antenna_tuner),
        sep(),
        annunciator("ANT 1", state.antenna == 1),
        sep(),
        annunciator("ANT 2", state.antenna == 2),
        sep(),
        annunciator("ATT", state.attenuator),
        sep(),
        annunciator("PRE-AMP", state.preamp),
        sep(),
        annunciator("VOX", state.vox),
        sep(),
        annunciator("PROC", state.proc),
        sep(),
        annunciator("NB", state.noise_blanker),
        sep(),
        annunciator("SPLIT", state.split),
        sep(),
        annunciator("FAST", state.fast_agc),
        sep(),
        annunciator("RIT", state.rit),
        sep(),
        annunciator("XIT", state.xit),
        sep(),
        annunciator("TX EQ.", state.tx_eq),
        sep(),
        annunciator("N.R. 1", state.noise_reduction == 1),
        sep(),
        annunciator("N.R. 2", state.noise_reduction == 2),
        sep(),
        annunciator("BEAT CANCEL", state.beat_cancel),
        sep(),
        annunciator("MENU", state.menu_mode),
        sep(),
        annunciator("M.CH", state.memory_scroll),
    ];
    let row1 = Paragraph::new(Line::from(row1_spans));
    f.render_widget(row1, rows[0]);

    // ------------------------------------------------------------------ Row 2
    // VFO badge + large frequency.
    let vfo_badge = match state.active_vfo {
        VfoSel::A => "A►",
        VfoSel::B => "B►",
        VfoSel::Memory => "M►",
    };
    let freq_str = format_freq(state);
    let row2_spans = vec![
        Span::styled(
            vfo_badge,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            freq_str,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  MHz"),
    ];
    let row2 = Paragraph::new(Line::from(row2_spans));
    f.render_widget(row2, rows[1]);

    // ------------------------------------------------------------------ Row 3
    // Meter bargraph.
    let meter_label = if state.tx { " PWR" } else { " S  " };

    // Bargraph fill ratio.
    let ratio = if state.tx {
        state.power_control as f64 / 100.0
    } else {
        // smeter: 0–30 maps to S0–S9+60 on a 0.0–1.0 scale.
        state.smeter as f64 / 30.0
    };

    // Leave room for label (4) + scale labels (~24) on the right; use remaining width.
    let bar_width = rows[2]
        .width
        .saturating_sub(4 + 25) as usize;
    let bar_str = bargraph(ratio, bar_width.max(1));
    let scale = "S1 S3 S5 S7 S9 +20 +40 +60";

    let row3_spans = vec![
        Span::styled(meter_label, on_style()),
        Span::styled(bar_str, Style::default().fg(Color::Yellow)),
        Span::styled(scale, off_style()),
    ];
    let row3 = Paragraph::new(Line::from(row3_spans));
    f.render_widget(row3, rows[2]);

    // ------------------------------------------------------------------ Row 4
    // Mode indicators + bottom annunciators.
    let mode_labels: &[(&str, bool)] = &[
        ("LSB", state.mode == 1),
        ("USB", state.mode == 2),
        ("CW", state.mode == 3),
        ("R", state.mode == 7 || state.mode == 9), // CW-R or FSK-R
        ("FSK", state.mode == 6),
        ("FM", state.mode == 4),
        ("AM", state.mode == 5),
    ];

    let mut row4_spans: Vec<Span> = Vec::new();
    for (label, active) in mode_labels {
        row4_spans.push(annunciator(label, *active));
        row4_spans.push(sep());
    }
    // Spacer between mode group and bottom annunciators.
    row4_spans.push(Span::raw("  "));

    let bottom_ann: &[(&str, bool)] = &[
        ("M.SCR", state.memory_scroll),
        ("F.LOCK", state.freq_lock),
        ("FINE", state.fine_step),
        ("1MHz", state.mhz_step),
        ("T", state.subtone),
        ("CTCSS", state.ctcss),
        ("CTRL", state.ctrl),
    ];
    for (label, active) in bottom_ann {
        row4_spans.push(annunciator(label, *active));
        row4_spans.push(sep());
    }

    let row4 = Paragraph::new(Line::from(row4_spans));
    f.render_widget(row4, rows[3]);

    // ------------------------------------------------------------------ Row 5
    // 2-digit auxiliary display.
    let aux_value = if state.menu_mode {
        state.menu_number
    } else {
        state.mem_channel
    };

    let mscr_label = if state.memory_scroll { "M.SCR " } else { "      " };
    let aux_str = format!("{}{:02}", mscr_label, aux_value);
    let row5 = Paragraph::new(Span::styled(
        aux_str,
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ));
    f.render_widget(row5, rows[4]);
}
