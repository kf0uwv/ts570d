use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Gauge, Paragraph},
    Frame,
};

pub fn draw_ui(f: &mut Frame) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(f.size());

    // VFO/frequency panel
    let vfo_text = "VFO A: 14.230.000 MHz    Mode: USB\nVFO B: 14.200.000 MHz";
    f.render_widget(
        Paragraph::new(vfo_text).block(Block::default().title("Frequency").borders(Borders::ALL)),
        chunks[0],
    );

    // S-meter panel
    f.render_widget(
        Gauge::default()
            .block(Block::default().title("S-Meter").borders(Borders::ALL))
            .gauge_style(Style::default().fg(Color::Green))
            .ratio(0.4)
            .label("S5"),
        chunks[1],
    );

    // Status bar
    f.render_widget(
        Paragraph::new("TS-570D Radio Control  |  q: quit")
            .block(Block::default().title("Status").borders(Borders::ALL)),
        chunks[2],
    );
}
