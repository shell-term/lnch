use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

pub fn render_status_bar(frame: &mut Frame, area: Rect) {
    let help_text = Line::from(vec![
        Span::styled("[a]", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" All Start  "),
        Span::styled("[s]", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" Start/Stop  "),
        Span::styled("[r]", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" Restart  "),
        Span::styled("[↑↓]", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" Select  "),
        Span::styled("[q]", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" Quit"),
    ]);

    let bar =
        Paragraph::new(help_text).style(Style::default().bg(Color::DarkGray).fg(Color::White));

    frame.render_widget(bar, area);
}
