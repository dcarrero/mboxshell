//! Search bar widget that appears at the bottom when search is active.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::i18n;
use crate::tui::app::App;
use crate::tui::theme::current_theme;

/// Render the search input bar with result counter and history indicator.
///
/// When the query is empty the prompt shows an inline syntax cheatsheet
/// (dimmed) so users discover field operators without reading docs.
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let theme = current_theme();

    const PROMPT: &str = " /: ";
    let mut spans: Vec<Span<'static>> = vec![Span::styled(PROMPT, theme.search_prompt)];

    if app.search_query.is_empty() && app.search_history_index.is_none() {
        // Dimmed cheatsheet after the (real) cursor
        spans.push(Span::styled(
            format!("  {}", i18n::tui_search_hint()),
            theme.help_dim,
        ));
    } else {
        spans.push(Span::styled(app.search_query.clone(), theme.message_body));

        // Result counter: (visible / total)
        let visible = app.visible_indices.len();
        let total = app.entries.len();
        let counter = format!(" ({visible} / {total})");
        spans.push(Span::styled(counter, theme.help_dim));
    }

    // History indicator
    if let Some(idx) = app.search_history_index {
        let hist_len = app.search_history.len();
        let indicator = format!("  [{} {}/{}]", i18n::tui_history(), idx + 1, hist_len);
        spans.push(Span::styled(indicator, theme.help_dim));
    }

    let line = Line::from(spans);
    let bar = Paragraph::new(line).style(theme.status_bar);
    frame.render_widget(bar, area);

    // The terminal's own cursor at the insertion point (it used to be a drawn
    // `_`, invisible to screen readers and braille displays).
    let col = PROMPT.width() + app.search_query.width();
    let x = area
        .x
        .saturating_add(col as u16)
        .min(area.right().saturating_sub(1));
    frame.set_cursor_position((x, area.y));
}
