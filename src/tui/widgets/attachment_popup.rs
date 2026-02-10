//! Attachment list popup for the selected message.

use ratatui::layout::{Constraint, Rect};
use ratatui::widgets::{Block, Borders, Cell, Clear, Row, Table};
use ratatui::Frame;

use crate::i18n;
use crate::tui::app::App;
use crate::tui::theme::current_theme;

/// Render the attachment popup centered on screen.
pub fn render(frame: &mut Frame, app: &App) {
    let theme = current_theme();
    let area = centered_rect(60, 50, frame.area());

    // Clear the area behind the popup
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.popup_title)
        .title(i18n::tui_attachments_title())
        .style(theme.popup);

    let attachments = app
        .current_body
        .as_ref()
        .map(|b| &b.attachments[..])
        .unwrap_or(&[]);

    if attachments.is_empty() {
        let rows = vec![Row::new(vec![
            Cell::from(i18n::tui_no_attachments()).style(theme.popup)
        ])];
        let table = Table::new(rows, [Constraint::Min(30)]).block(block);
        frame.render_widget(table, area);
        return;
    }

    let rows: Vec<Row> = attachments
        .iter()
        .enumerate()
        .map(|(i, att)| {
            let size = humansize::format_size(att.size, humansize::BINARY);
            let selected = i == app.attachment_selected;
            let marker = if selected { ">" } else { " " };
            let style = if selected {
                theme.list_selected
            } else {
                theme.popup
            };
            let name_style = if selected {
                theme.list_selected
            } else {
                theme.attachment
            };
            Row::new(vec![
                Cell::from(marker).style(style),
                Cell::from(format!("{}", i + 1)).style(style),
                Cell::from(att.filename.clone()).style(name_style),
                Cell::from(att.content_type.clone()).style(style),
                Cell::from(size).style(style),
            ])
        })
        .collect();

    let header = Row::new(vec![
        Cell::from("").style(theme.popup_title),
        Cell::from("#").style(theme.popup_title),
        Cell::from(i18n::tui_col_filename()).style(theme.popup_title),
        Cell::from(i18n::tui_col_type()).style(theme.popup_title),
        Cell::from(i18n::tui_col_size()).style(theme.popup_title),
    ]);

    let footer = vec![
        Row::new(vec![
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
        ]),
        Row::new(vec![
            Cell::from(""),
            Cell::from(i18n::tui_attachment_footer()).style(theme.status_bar),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
        ]),
    ];

    let all_rows: Vec<Row> = rows.into_iter().chain(footer).collect();

    let table = Table::new(
        all_rows,
        [
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(20),
            Constraint::Length(20),
            Constraint::Length(10),
        ],
    )
    .header(header)
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
