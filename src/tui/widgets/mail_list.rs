//! Mail list widget — virtual-scrolling table of messages.

use ratatui::layout::{Constraint, Rect};
use ratatui::style::Modifier;
use ratatui::widgets::{Block, Borders, Row, Table};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::tui::app::{App, PanelFocus, SortColumn};
use crate::tui::theme::current_theme;

/// Render the message list table with virtual scrolling.
pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = current_theme();

    let is_focused = app.focus == PanelFocus::MailList;
    let border_style = if is_focused {
        theme.border.add_modifier(Modifier::BOLD)
    } else {
        theme.border
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(" Messages ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 2 {
        return;
    }

    // Header row takes 1 line, rest is data
    let viewport_height = (inner.height as usize).saturating_sub(1);
    app.list_viewport_height = viewport_height;

    // Column widths
    let mark_w = 2u16;
    let date_w = 17u16;
    let size_w = 8u16;
    let att_w = 2u16;
    let from_w = 20u16.min(inner.width / 4);
    let fixed = mark_w + date_w + from_w + size_w + att_w + 5; // 5 for padding
    let subject_w = inner.width.saturating_sub(fixed);

    let constraints = [
        Constraint::Length(mark_w),
        Constraint::Length(date_w),
        Constraint::Length(from_w),
        Constraint::Min(subject_w),
        Constraint::Length(size_w),
        Constraint::Length(att_w),
    ];

    // Sort indicator
    let sort_arrow = |col: SortColumn| -> &str {
        if app.sort_column == col {
            if app.sort_ascending {
                " ^"
            } else {
                " v"
            }
        } else {
            ""
        }
    };

    let h_date = format!("Date{}", sort_arrow(SortColumn::Date));
    let h_from = format!("From{}", sort_arrow(SortColumn::From));
    let h_subject = format!("Subject{}", sort_arrow(SortColumn::Subject));
    let h_size = format!("Size{}", sort_arrow(SortColumn::Size));

    let header = Row::new(vec![
        " ".to_string(),
        h_date,
        h_from,
        h_subject,
        h_size,
        String::new(),
    ])
    .style(theme.list_header);

    // Virtual scrolling: only build rows for visible range
    let start = app.list_scroll_offset;
    let end = (start + viewport_height).min(app.visible_count());

    let rows: Vec<Row> = (start..end)
        .map(|vis_idx| {
            let real_idx = app.visible_indices[vis_idx];
            let entry = &app.entries[real_idx];

            let is_selected = vis_idx == app.selected;
            let is_marked = app.marked.contains(&entry.offset);

            let mark = if is_marked { "*" } else { " " };
            let date = entry.date.format("%Y-%m-%d %H:%M").to_string();

            let from_display = if entry.from.display_name.is_empty() {
                entry.from.address.clone()
            } else {
                entry.from.display_name.clone()
            };
            let from_truncated = truncate_str(&from_display, from_w as usize);

            // Indent subject in threaded view
            let depth = app.thread_depth(vis_idx);
            let indent = if depth > 0 {
                let arrows = "\u{2514} "; // └
                let prefix: String = "  ".repeat(depth.saturating_sub(1));
                format!("{prefix}{arrows}")
            } else {
                String::new()
            };
            let avail_subj = (subject_w as usize).saturating_sub(indent.len());
            let subject_truncated = format!("{indent}{}", truncate_str(&entry.subject, avail_subj));

            let size = humansize::format_size(entry.length, humansize::BINARY);
            let att = if entry.has_attachments { "@" } else { " " };

            let style = if is_selected {
                theme.list_selected
            } else if is_marked {
                theme.list_marked
            } else {
                theme.list_normal
            };

            Row::new(vec![
                mark.to_string(),
                date,
                from_truncated,
                subject_truncated,
                size,
                att.to_string(),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(rows, constraints)
        .header(header)
        .column_spacing(1);

    frame.render_widget(table, inner);
}

/// Truncate a string to fit within `max_width` columns, adding "..." if needed.
fn truncate_str(s: &str, max_width: usize) -> String {
    let width = UnicodeWidthStr::width(s);
    if width <= max_width {
        s.to_string()
    } else if max_width <= 3 {
        s.chars().take(max_width).collect()
    } else {
        let mut result = String::new();
        let mut current_width = 0;
        for ch in s.chars() {
            let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if current_width + ch_width + 3 > max_width {
                break;
            }
            result.push(ch);
            current_width += ch_width;
        }
        result.push_str("...");
        result
    }
}
