//! Sidebar widget showing labels/folders for filtering messages.

use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::tui::app::{App, PanelFocus};
use crate::tui::theme::current_theme;

/// Render the label sidebar panel.
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let theme = current_theme();

    let is_focused = app.focus == PanelFocus::Sidebar;
    let border_style = if is_focused {
        theme.border.add_modifier(Modifier::BOLD)
    } else {
        theme.border
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(" Labels ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 1 || inner.width < 4 {
        return;
    }

    let max_width = inner.width as usize;
    let mut lines: Vec<Line> = Vec::new();

    // "All Messages" entry (index 0 in sidebar)
    let all_count = app.entries.len();
    let is_selected = app.sidebar_selected == 0;
    let is_active = app.active_label_filter.is_none();
    let all_label = truncate_sidebar_entry("All Messages", all_count, max_width);

    let style = if is_selected && is_focused {
        theme.sidebar_selected
    } else if is_active {
        theme.sidebar_selected.remove_modifier(Modifier::BOLD)
    } else {
        theme.sidebar
    };
    lines.push(Line::from(Span::styled(all_label, style)));

    // Separator
    if inner.height > 2 {
        lines.push(Line::from(Span::styled(
            "\u{2500}".repeat(max_width.min(40)),
            theme.border,
        )));
    }

    // Label entries (index 1.. in sidebar)
    for (i, label) in app.all_labels.iter().enumerate() {
        let count = app.label_counts[i];
        let sidebar_idx = i + 1; // offset by 1 because of "All Messages"
        let is_selected = app.sidebar_selected == sidebar_idx;
        let is_active = app
            .active_label_filter
            .as_ref()
            .map(|l| l == label)
            .unwrap_or(false);

        let entry_text = truncate_sidebar_entry(label, count, max_width);

        let style = if is_selected && is_focused {
            theme.sidebar_selected
        } else if is_active {
            theme.sidebar_selected.remove_modifier(Modifier::BOLD)
        } else {
            theme.sidebar
        };

        lines.push(Line::from(Span::styled(entry_text, style)));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Format a sidebar entry as "Label Name  (123)" truncated to fit.
fn truncate_sidebar_entry(label: &str, count: usize, max_width: usize) -> String {
    let count_str = format!(" ({count})");
    let avail = max_width.saturating_sub(count_str.len());
    if label.len() <= avail {
        format!(" {label}{}{count_str}", " ".repeat(avail - label.len()))
    } else if avail > 3 {
        format!(" {}...{count_str}", &label[..avail - 3])
    } else {
        format!(" {label}")
    }
}
