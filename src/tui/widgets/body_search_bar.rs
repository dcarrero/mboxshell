//! In-body search bar shown at the top of the message panel while searching
//! within a message, next to the body being searched.
//!
//! Mirrors the global [`search_bar`](super::search_bar) but scopes the query to
//! the currently open message body, with a match counter and navigation hints.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::i18n;
use crate::tui::app::App;
use crate::tui::theme::current_theme;

/// Render the in-body search prompt with a match counter and navigation hints.
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let theme = current_theme();

    const PROMPT: &str = " /";
    let mut spans: Vec<Span<'static>> = vec![Span::styled(PROMPT, theme.search_prompt)];
    spans.push(Span::styled(
        app.body_search_query.clone(),
        theme.message_body,
    ));

    if app.body_search_query.is_empty() {
        spans.push(Span::styled(
            format!("  {}", i18n::tui_body_search_hint()),
            theme.help_dim,
        ));
    } else if app.body_search_matches.is_empty() {
        spans.push(Span::styled(
            format!("  {}", i18n::tui_body_search_no_match()),
            theme.help_dim,
        ));
    } else {
        let counter = format!(
            "  ({}/{})  {}",
            app.body_search_index + 1,
            app.body_search_matches.len(),
            i18n::tui_body_search_nav_hint(),
        );
        spans.push(Span::styled(counter, theme.help_dim));
    }

    let bar = Paragraph::new(Line::from(spans)).style(theme.status_bar);
    frame.render_widget(bar, area);

    // Real terminal cursor at the insertion point (see `search_bar`).
    let col = PROMPT.width() + app.body_search_query.width();
    let x = area
        .x
        .saturating_add(col as u16)
        .min(area.right().saturating_sub(1));
    frame.set_cursor_position((x, area.y));
}
