//! Bottom status bar showing transient messages or keyboard hints.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::theme::current_theme;

/// Render the status bar at the bottom.
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let theme = current_theme();

    let content = if let Some((msg, _)) = &app.status_message {
        Line::from(Span::styled(format!(" {msg}"), theme.status_bar))
    } else {
        let mut hints = vec![
            Span::styled(" j/k", theme.search_prompt),
            Span::styled(":Nav  ", theme.status_bar),
            Span::styled("/", theme.search_prompt),
            Span::styled(":Search  ", theme.status_bar),
            Span::styled("Enter", theme.search_prompt),
            Span::styled(":Open  ", theme.status_bar),
            Span::styled("s", theme.search_prompt),
            Span::styled(":Sort  ", theme.status_bar),
            Span::styled("e", theme.search_prompt),
            Span::styled(":Export  ", theme.status_bar),
            Span::styled("a", theme.search_prompt),
            Span::styled(":Attach  ", theme.status_bar),
        ];
        if !app.all_labels.is_empty() {
            hints.push(Span::styled("L", theme.search_prompt));
            hints.push(Span::styled(":Labels  ", theme.status_bar));
        }
        hints.push(Span::styled("?", theme.search_prompt));
        hints.push(Span::styled(":Help  ", theme.status_bar));
        hints.push(Span::styled("q", theme.search_prompt));
        hints.push(Span::styled(":Quit", theme.status_bar));
        Line::from(hints)
    };

    let bar = Paragraph::new(content).style(theme.status_bar);
    frame.render_widget(bar, area);
}
