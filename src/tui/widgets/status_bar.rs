use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

pub fn render_status_bar(frame: &mut Frame, area: Rect, update_available: bool, confirm_quit: bool) {
    if confirm_quit {
        let spans = vec![
            Span::styled(" Processes are still running. ", Style::default().fg(Color::Yellow)),
            Span::raw("Quit? "),
            Span::styled("[y]", Style::default().fg(Color::Red).bold()),
            Span::raw(" Yes  "),
            Span::styled("[any]", Style::default().fg(Color::Green).bold()),
            Span::raw(" Cancel"),
        ];
        let bar = Paragraph::new(Line::from(spans))
            .style(Style::default().bg(Color::DarkGray).fg(Color::White));
        frame.render_widget(bar, area);
        return;
    }

    let mut spans = vec![
        Span::styled("[a]", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" All Start  "),
        Span::styled("[s]", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" Start/Stop  "),
        Span::styled("[r]", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" Restart  "),
        Span::styled("[↑↓]", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" Select  "),
        Span::styled("[c]", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" Clear  "),
    ];

    if update_available {
        spans.push(Span::styled("[u]", Style::default().fg(Color::Green).bold()));
        spans.push(Span::raw(" Update  "));
    }

    spans.push(Span::styled("[q]", Style::default().fg(Color::Yellow).bold()));
    spans.push(Span::raw(" Quit"));

    let bar = Paragraph::new(Line::from(spans))
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));

    frame.render_widget(bar, area);
}
