//! Export options popup for the selected message.

use ratatui::layout::{Constraint, Rect};
use ratatui::widgets::{Block, Borders, Cell, Clear, Row, Table};
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::theme::current_theme;

/// Available export options.
pub const EXPORT_OPTIONS: &[(&str, &str)] = &[
    ("EML", "Raw email (RFC 5322 .eml file)"),
    ("TXT", "Plain text with headers"),
    ("CSV", "Metadata summary (CSV)"),
    ("Attachments", "Save all attachments to folder"),
];

/// Render the export popup centered on screen.
pub fn render(frame: &mut Frame, app: &App) {
    let theme = current_theme();
    let area = centered_rect(50, 40, frame.area());

    frame.render_widget(Clear, area);

    let has_marked = !app.marked.is_empty();
    let title = if has_marked {
        format!(" Export {} marked message(s) ", app.marked.len())
    } else {
        " Export current message ".to_string()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.popup_title)
        .title(title)
        .style(theme.popup);

    let rows: Vec<Row> = EXPORT_OPTIONS
        .iter()
        .enumerate()
        .map(|(i, (name, desc))| {
            let marker = if i == app.export_selected {
                ">"
            } else {
                " "
            };
            let style = if i == app.export_selected {
                theme.list_selected
            } else {
                theme.popup
            };
            Row::new(vec![
                Cell::from(marker).style(style),
                Cell::from(*name).style(style),
                Cell::from(*desc).style(style),
            ])
        })
        .collect();

    let footer_rows = vec![
        Row::new(vec![Cell::from(""), Cell::from(""), Cell::from("")]),
        Row::new(vec![
            Cell::from(""),
            Cell::from("j/k:Navigate  Enter:Export  Esc:Cancel").style(theme.status_bar),
            Cell::from(""),
        ]),
    ];

    let all_rows: Vec<Row> = rows.into_iter().chain(footer_rows).collect();

    let table = Table::new(
        all_rows,
        [
            Constraint::Length(2),
            Constraint::Length(14),
            Constraint::Min(20),
        ],
    )
    .block(block)
    .column_spacing(1);

    frame.render_widget(table, area);
}

/// Calculate a centered rectangle.
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let width = area.width * percent_x / 100;
    let height = area.height * percent_y / 100;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}
