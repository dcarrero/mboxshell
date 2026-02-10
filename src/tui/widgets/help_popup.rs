//! Help popup showing all keyboard shortcuts.

use ratatui::layout::{Constraint, Rect};
use ratatui::widgets::{Block, Borders, Cell, Clear, Row, Table};
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::theme::current_theme;

/// Render the help popup centered on screen.
pub fn render(frame: &mut Frame, _app: &App) {
    let theme = current_theme();
    let area = centered_rect(70, 80, frame.area());

    // Clear the area behind the popup
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.popup_title)
        .title(" Keyboard Shortcuts ")
        .style(theme.popup);

    let shortcuts = vec![
        ("Navigation", ""),
        ("j / Down", "Next item / scroll down"),
        ("k / Up", "Previous item / scroll up"),
        ("g / Home", "First item"),
        ("G / End", "Last item"),
        ("PgDn / PgUp", "Page down / up"),
        ("Enter", "Open message / select label"),
        ("Tab / S-Tab", "Cycle panel focus"),
        ("Esc", "Back to list / close popup"),
        ("", ""),
        ("Layout", ""),
        ("1", "List only"),
        ("2", "Horizontal split (list + view)"),
        ("3", "Vertical split (list + view)"),
        ("", ""),
        ("Actions", ""),
        ("Space", "Mark / unmark message"),
        ("*", "Mark / unmark all"),
        ("s", "Cycle sort column"),
        ("S", "Toggle sort direction"),
        ("h", "Toggle full headers"),
        ("r", "Toggle raw message source"),
        ("a", "Show attachments (j/k, Enter:save, A:all)"),
        ("e", "Export message (EML, TXT, CSV)"),
        ("t", "Toggle threaded view"),
        ("", ""),
        ("Labels / Sidebar", ""),
        ("L", "Open / focus / close sidebar"),
        ("j/k", "Navigate labels"),
        ("Enter", "Filter by label + go to list"),
        ("Esc", "Back to list"),
        ("", ""),
        ("Search", ""),
        ("/", "Open search bar"),
        ("n", "Next search result"),
        ("N", "Previous search result"),
        ("", ""),
        ("General", ""),
        ("?", "Toggle this help"),
        ("q", "Quit (from list, view, sidebar)"),
        ("Ctrl-C", "Quit (from anywhere)"),
    ];

    let rows: Vec<Row> = shortcuts
        .iter()
        .map(|(key, desc)| {
            if desc.is_empty() && !key.is_empty() {
                // Section header
                Row::new(vec![
                    Cell::from(*key).style(theme.popup_title),
                    Cell::from(""),
                ])
                .bottom_margin(0)
            } else if key.is_empty() {
                // Blank separator
                Row::new(vec![Cell::from(""), Cell::from("")])
            } else {
                Row::new(vec![
                    Cell::from(*key).style(theme.search_prompt),
                    Cell::from(*desc).style(theme.popup),
                ])
            }
        })
        .collect();

    let table = Table::new(rows, [Constraint::Length(16), Constraint::Min(30)])
        .block(block)
        .column_spacing(2);

    frame.render_widget(table, area);
}

/// Calculate a centered rectangle with given percentage of width and height.
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let width = area.width * percent_x / 100;
    let height = area.height * percent_y / 100;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}
