//! Top header bar showing file info and message count.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::i18n;
use crate::tui::app::App;
use crate::tui::theme::current_theme;

/// Render the top header bar.
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let theme = current_theme();

    // `file_name()` is not a usable label for every mailbox: Apple Mail's is
    // literally `mbox`, and a Google Groups export's is the localised
    // `topics.mbox` / `temas.mbox`. Both are named by their directory instead.
    let file_name = crate::mailbox_naming::display_name(&app.mbox_path);

    let total = app.entries.len();
    let visible = app.visible_count();
    let marked = app.marked.len();

    let mut spans = vec![
        Span::styled(format!(" {file_name}"), theme.header_bar),
        Span::styled(
            format!(" | {visible} / {total} {}", i18n::tui_messages_count()),
            theme.header_bar,
        ),
    ];

    if marked > 0 {
        spans.push(Span::styled(
            format!(" | {marked} {}", i18n::tui_marked_count()),
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
    let right_text = i18n::tui_help_hint();
    let padding = (area.width as usize)
        .saturating_sub(left_len)
        .saturating_sub(right_text.len());
    if padding > 0 {
        spans.push(Span::styled(" ".repeat(padding), theme.header_bar));
    }
    spans.push(Span::styled(right_text, theme.header_bar));

    let line = Line::from(spans);
    let bar = Paragraph::new(line).style(theme.header_bar);
    frame.render_widget(bar, area);
}
