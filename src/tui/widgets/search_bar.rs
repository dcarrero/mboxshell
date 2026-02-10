//! Search bar widget that appears at the bottom when search is active.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::theme::current_theme;

/// Render the search input bar with result counter and history indicator.
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let theme = current_theme();

    let mut spans: Vec<Span<'static>> = vec![
        Span::styled(" /: ", theme.search_prompt),
        Span::styled(app.search_query.clone(), theme.message_body),
        Span::styled("_", theme.search_prompt), // cursor indicator
    ];

    // Result counter: (visible / total)
    let visible = app.visible_indices.len();
    let total = app.entries.len();
    let counter = format!(" ({visible} / {total})");
    spans.push(Span::styled(counter, theme.help_dim));

    // History indicator
    if let Some(idx) = app.search_history_index {
        let hist_len = app.search_history.len();
        let indicator = format!("  [history {}/{}]", idx + 1, hist_len);
        spans.push(Span::styled(indicator, theme.help_dim));
    }

    let line = Line::from(spans);
    let bar = Paragraph::new(line).style(theme.status_bar);
    frame.render_widget(bar, area);
}
