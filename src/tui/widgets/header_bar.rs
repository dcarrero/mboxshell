//! Top header bar showing file info and message count.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::theme::current_theme;

/// Render the top header bar.
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let theme = current_theme();

    let file_name = app
        .mbox_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| app.mbox_path.to_string_lossy().to_string());

    let total = app.entries.len();
    let visible = app.visible_count();
    let marked = app.marked.len();

    let mut spans = vec![
        Span::styled(format!(" {file_name}"), theme.header_bar),
        Span::styled(format!(" | {visible} / {total} messages"), theme.header_bar),
    ];

    if marked > 0 {
        spans.push(Span::styled(
            format!(" | {marked} marked"),
            theme.header_bar,
        ));
    }

    if let Some(label) = &app.active_label_filter {
        spans.push(Span::styled(format!(" | label: {label}"), theme.header_bar));
    }

    if !app.search_query.is_empty() && !app.search_active {
        spans.push(Span::styled(
            format!(" | search: \"{}\"", app.search_query),
            theme.header_bar,
        ));
    }

    // Right-aligned help hint
    let left_len: usize = spans.iter().map(|s| s.content.len()).sum();
    let right_text = " [?] Help ";
    let padding = area.width as usize - left_len.min(area.width as usize) - right_text.len();
    if padding > 0 && area.width as usize > left_len + right_text.len() {
        spans.push(Span::styled(" ".repeat(padding), theme.header_bar));
    }
    spans.push(Span::styled(right_text, theme.header_bar));

    let line = Line::from(spans);
    let bar = Paragraph::new(line).style(theme.header_bar);
    frame.render_widget(bar, area);
}
