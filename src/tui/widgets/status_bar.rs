//! Bottom status bar showing transient messages or context-sensitive keyboard hints.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::i18n;
use crate::tui::app::{App, PanelFocus};
use crate::tui::theme::current_theme;

/// Version string shown at the right edge of the status bar.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Render the status bar at the bottom with context-sensitive hints and version.
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let theme = current_theme();

    let version_text = format!("v{VERSION} ");
    let version_width = version_text.len() as u16;

    // Split: hints (flexible) | version (fixed)
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(10), Constraint::Length(version_width)])
        .split(area);

    // Left side: hints or status message
    let content = if let Some((msg, _)) = &app.status_message {
        Line::from(Span::styled(format!(" {msg}"), theme.status_bar))
    } else {
        let hints = build_hints(app);
        let mut spans = Vec::new();
        for (i, (key, desc)) in hints.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" ", theme.status_bar));
            }
            spans.push(Span::styled(format!(" {key}"), theme.search_prompt));
            spans.push(Span::styled(format!(":{desc}"), theme.status_bar));
        }
        Line::from(spans)
    };

    let bar = Paragraph::new(content).style(theme.status_bar);
    frame.render_widget(bar, chunks[0]);

    // Right side: version
    let version = Paragraph::new(Line::from(Span::styled(version_text, theme.border)))
        .alignment(Alignment::Right)
        .style(theme.status_bar);
    frame.render_widget(version, chunks[1]);
}

/// Return context-sensitive hint pairs (key, description) for the active panel.
fn build_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    let mut hints = Vec::new();

    match app.focus {
        PanelFocus::Sidebar => {
            hints.push(("j/k", i18n::tui_hint_nav()));
            hints.push(("Enter", i18n::tui_hint_select()));
            if !app.all_labels.is_empty() {
                hints.push(("L", i18n::tui_hint_labels()));
            }
            hints.push(("Esc", i18n::tui_hint_back()));
            hints.push(("Tab", i18n::tui_hint_panel()));
            hints.push(("?", i18n::tui_hint_help()));
            hints.push(("q", i18n::tui_hint_quit()));
        }
        PanelFocus::MailList => {
            hints.push(("j/k", i18n::tui_hint_nav()));
            hints.push(("/", i18n::tui_hint_search()));
            hints.push(("Enter", i18n::tui_hint_open()));
            hints.push(("s", i18n::tui_hint_sort()));
            hints.push(("Space", i18n::tui_hint_mark()));
            hints.push(("e", i18n::tui_hint_export()));
            hints.push(("a", i18n::tui_hint_attach()));
            hints.push(("t", i18n::tui_hint_thread()));
            if !app.all_labels.is_empty() {
                hints.push(("L", i18n::tui_hint_labels()));
            }
            hints.push(("Tab", i18n::tui_hint_panel()));
            hints.push(("?", i18n::tui_hint_help()));
            hints.push(("q", i18n::tui_hint_quit()));
        }
        PanelFocus::MailView => {
            hints.push(("j/k", i18n::tui_hint_scroll()));
            hints.push(("h", i18n::tui_hint_headers()));
            hints.push(("r", i18n::tui_hint_raw()));
            hints.push(("e", i18n::tui_hint_export()));
            hints.push(("a", i18n::tui_hint_attach()));
            hints.push(("Esc", i18n::tui_hint_back()));
            hints.push(("Tab", i18n::tui_hint_panel()));
            hints.push(("?", i18n::tui_hint_help()));
            hints.push(("q", i18n::tui_hint_quit()));
        }
        PanelFocus::SearchBar => {
            hints.push(("Enter", i18n::tui_hint_search()));
            hints.push(("Esc", i18n::tui_hint_cancel()));
        }
    }

    hints
}
