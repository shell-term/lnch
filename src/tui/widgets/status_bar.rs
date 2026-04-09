use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::tui::app::StatusFeedback;

pub fn render_status_bar(frame: &mut Frame, area: Rect, update_available: bool, confirm_quit: bool, show_copied: bool, show_selected: bool, status_feedback: Option<&StatusFeedback>) {
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
        Span::styled("[a/A]", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" Start/Stop All  "),
        Span::styled("[s]", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" Start/Stop  "),
        Span::styled("[r]", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" Restart  "),
        Span::styled("[↑↓]", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" Select  "),
        Span::styled("[/]", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" Search  "),
        Span::styled("[c]", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" Clear  "),
    ];

    if update_available {
        spans.push(Span::styled("[u]", Style::default().fg(Color::Green).bold()));
        spans.push(Span::raw(" Update  "));
    }

    spans.push(Span::styled("[q]", Style::default().fg(Color::Yellow).bold()));
    spans.push(Span::raw(" Quit"));

    if let Some(fb) = status_feedback {
        spans.push(Span::raw("  "));
        let style = if fb.is_error {
            Style::default().fg(Color::White).bg(Color::Red).bold()
        } else {
            Style::default().fg(Color::Black).bg(Color::Green).bold()
        };
        spans.push(Span::styled(format!(" {} ", fb.message), style));
    } else if show_copied {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            " Copied! ",
            Style::default().fg(Color::Black).bg(Color::Green).bold(),
        ));
    } else if show_selected {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            "[Ctrl+C]",
            Style::default().fg(Color::Cyan).bold(),
        ));
        spans.push(Span::raw(" Copy"));
    }

    let bar = Paragraph::new(Line::from(spans))
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));

    frame.render_widget(bar, area);
}
