//! Search bar widget that appears at the bottom when search is active.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::theme::current_theme;

/// Render the search input bar.
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let theme = current_theme();

    let line = Line::from(vec![
        Span::styled(" /: ", theme.search_prompt),
        Span::styled(&app.search_query, theme.message_body),
        Span::styled("_", theme.search_prompt), // cursor indicator
    ]);

    let bar = Paragraph::new(line).style(theme.status_bar);
    frame.render_widget(bar, area);
}
