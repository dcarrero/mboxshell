//! Export options popup for the selected message.

use ratatui::layout::{Constraint, Rect};
use ratatui::widgets::{Block, Borders, Cell, Clear, Row, Table};
use ratatui::Frame;

use crate::i18n;
use crate::tui::app::App;
use crate::tui::theme::current_theme;

/// Number of export options available.
pub const EXPORT_OPTION_COUNT: usize = 4;

/// Available export options (localized).
pub fn export_options() -> Vec<(&'static str, &'static str)> {
    vec![
        (i18n::tui_export_eml(), i18n::tui_export_eml_desc()),
        (i18n::tui_export_txt(), i18n::tui_export_txt_desc()),
        (i18n::tui_export_csv(), i18n::tui_export_csv_desc()),
        (
            i18n::tui_export_attachments(),
            i18n::tui_export_attachments_desc(),
        ),
    ]
}

/// Render the export popup centered on screen.
pub fn render(frame: &mut Frame, app: &App) {
    let theme = current_theme();
    let area = centered_rect(50, 40, frame.area());

    frame.render_widget(Clear, area);

    let has_marked = !app.marked.is_empty();
    let title = if has_marked {
        format!(
            " {} ({}) ",
            i18n::tui_export_marked_title(),
            app.marked.len()
        )
    } else {
        format!(" {} ", i18n::tui_export_current_title())
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.popup_title)
        .title(title)
        .style(theme.popup);

    let options = export_options();
    let rows: Vec<Row> = options
        .iter()
        .enumerate()
        .map(|(i, (name, desc))| {
            let marker = if i == app.export_selected { ">" } else { " " };
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
            Cell::from(i18n::tui_export_footer()).style(theme.status_bar),
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
