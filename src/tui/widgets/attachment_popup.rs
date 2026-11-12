//! Attachment list popup for the selected message.

use ratatui::layout::{Constraint, Rect};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table};
use ratatui::Frame;

use crate::i18n;
use crate::tui::app::App;
use crate::tui::text::sanitize_line;
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
        let inner = block.inner(area);
        let table = Table::new(rows, [Constraint::Min(30)]).block(block);
        frame.render_widget(table, area);
        super::park_cursor(frame, inner, 0);
        return;
    }

    // The table gets the popup minus its last line, which holds the key
    // hints. Only the rows that fit are drawn, scrolled so the selected one
    // is always among them: with more attachments than rows, the selection
    // used to move off-screen and Enter saved a file that was not in view.
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let footer_height = u16::from(inner.height > 2);
    let table_area = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(footer_height),
    );
    // One line of the table is the column header.
    let capacity = usize::from(table_area.height.saturating_sub(1)).max(1);
    let selected = app
        .attachment_selected
        .min(attachments.len().saturating_sub(1));
    let first = (selected + 1).saturating_sub(capacity);

    let rows: Vec<Row> = attachments
        .iter()
        .enumerate()
        .skip(first)
        .take(capacity)
        .map(|(i, att)| {
            let size = humansize::format_size(att.size, humansize::BINARY);
            let selected = i == selected;
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
                Cell::from(sanitize_line(&att.filename).into_owned()).style(name_style),
                Cell::from(sanitize_line(&att.content_type).into_owned()).style(style),
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

    let table = Table::new(
        rows,
        [
            Constraint::Length(2),
            Constraint::Length(4),
            Constraint::Min(20),
            Constraint::Length(20),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .column_spacing(1)
    .style(theme.popup);
    frame.render_widget(table, table_area);

    if footer_height > 0 {
        let footer_area = Rect::new(inner.x, inner.y + table_area.height, inner.width, 1);
        let position = format!(" {}/{}  ", selected + 1, attachments.len());
        let footer = Paragraph::new(format!("{position}{}", i18n::tui_attachment_footer()))
            .style(theme.status_bar);
        frame.render_widget(footer, footer_area);
    }

    // Row 0 is the column header.
    super::park_cursor(frame, table_area, 1 + (selected - first));
}

/// Calculate a centered rectangle.
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let width = super::percent_of(area.width, percent_x);
    let height = super::percent_of(area.height, percent_y);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}
