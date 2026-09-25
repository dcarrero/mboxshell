//! Bottom status bar showing transient messages or context-sensitive keyboard hints.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

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
        let (hints, cut) = fit_hints(build_hints(app), chunks[0].width as usize);
        let mut spans = Vec::new();
        for (i, (key, desc)) in hints.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" ", theme.status_bar));
            }
            spans.push(Span::styled(format!(" {key}"), theme.search_prompt));
            spans.push(Span::styled(format!(":{desc}"), theme.status_bar));
        }
        if cut {
            spans.push(Span::styled(" \u{2026}", theme.status_bar));
        }
        Line::from(spans)
    };

    let bar = Paragraph::new(content).style(theme.status_bar);
    frame.render_widget(bar, chunks[0]);

    // Right side: version
    let version = Paragraph::new(Line::from(Span::styled(version_text, theme.status_bar)))
        .alignment(Alignment::Right)
        .style(theme.status_bar);
    frame.render_widget(version, chunks[1]);
}

/// Keep the hints that fit in `width` columns, in order, always keeping the
/// trailing `?` (help) and `q` (quit) pairs. A plain cut used to drop exactly
/// those two first on a narrow terminal. Returns whether any were dropped.
fn fit_hints(
    hints: Vec<(&'static str, &'static str)>,
    width: usize,
) -> (Vec<(&'static str, &'static str)>, bool) {
    // " key:desc" plus the one-space separator before every pair but the first.
    let cost = |&(key, desc): &(&str, &str)| 1 + key.width() + 1 + desc.width() + 1;
    let keep_tail = hints
        .iter()
        .rev()
        .take_while(|(key, _)| matches!(*key, "?" | "q"))
        .count();
    let (head, tail) = hints.split_at(hints.len() - keep_tail);
    let ellipsis = 2;
    let mut budget = width.saturating_sub(tail.iter().map(cost).sum::<usize>());
    let fits_whole = head.iter().map(cost).sum::<usize>() <= budget;
    if !fits_whole {
        budget = budget.saturating_sub(ellipsis);
    }

    let mut kept = Vec::with_capacity(hints.len());
    // A long hint that does not fit is skipped, not a stop: shorter ones
    // after it may still fit, and they keep their order.
    for hint in head {
        if cost(hint) > budget {
            continue;
        }
        budget -= cost(hint);
        kept.push(*hint);
    }
    let cut = kept.len() < head.len();
    kept.extend_from_slice(tail);
    (kept, cut)
}

/// Return context-sensitive hint pairs (key, description) for the active panel.
fn build_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    let mut hints = Vec::new();

    match app.focus {
        PanelFocus::Sidebar => {
            hints.push(("j/k", i18n::tui_hint_nav()));
            hints.push(("Enter", i18n::tui_hint_select()));
            if !app.all_labels.is_empty() {
                hints.push(("l", i18n::tui_hint_labels()));
            }
            hints.push(("Esc", i18n::tui_hint_back()));
            hints.push(("Tab", i18n::tui_hint_panel()));
            hints.push(("?", i18n::tui_hint_help()));
            hints.push(("q", i18n::tui_hint_quit()));
        }
        PanelFocus::MailList => {
            hints.push(("j/k", i18n::tui_hint_nav()));
            hints.push(("/", i18n::tui_hint_search()));
            hints.push(("f", i18n::tui_hint_filters()));
            hints.push(("Enter", i18n::tui_hint_open()));
            hints.push(("Shift+\u{2191}\u{2193}", i18n::tui_hint_scroll_body()));
            hints.push(("s", i18n::tui_hint_sort()));
            hints.push(("Space", i18n::tui_hint_mark()));
            hints.push(("e", i18n::tui_hint_export()));
            hints.push(("a", i18n::tui_hint_attach()));
            hints.push(("t", i18n::tui_hint_thread()));
            if !app.all_labels.is_empty() {
                hints.push(("l", i18n::tui_hint_labels()));
            }
            hints.push(("Tab", i18n::tui_hint_panel()));
            hints.push(("?", i18n::tui_hint_help()));
            hints.push(("q", i18n::tui_hint_quit()));
        }
        PanelFocus::MailView => {
            hints.push(("j/k", i18n::tui_hint_scroll()));
            hints.push(("/", i18n::tui_hint_find()));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fit_hints_keeps_help_and_quit_on_narrow_bars() {
        let hints = vec![
            ("j/k", "Navigate"),
            ("/", "Search"),
            ("f", "Filters"),
            ("?", "Help"),
            ("q", "Quit"),
        ];
        let (all, cut) = fit_hints(hints.clone(), 200);
        assert_eq!((all.len(), cut), (5, false));

        let (few, cut) = fit_hints(hints, 34);
        assert!(cut);
        let keys: Vec<&str> = few.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys.last(), Some(&"q"));
        assert!(keys.contains(&"?"));
        assert!(keys.len() < 5);
    }
}
