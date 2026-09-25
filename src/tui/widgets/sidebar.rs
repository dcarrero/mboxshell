//! Sidebar widget showing labels/folders for filtering messages.

use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::i18n;
use crate::tui::app::{App, PanelFocus};
use crate::tui::theme::current_theme;

/// Render the label sidebar panel.
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let theme = current_theme();

    let is_focused = app.focus == PanelFocus::Sidebar;
    let border_style = if is_focused {
        theme.border_focused
    } else {
        theme.border
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(i18n::tui_labels_title());

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
    let all_label = truncate_sidebar_entry(
        row_marker(is_selected && is_focused, is_active),
        i18n::tui_all_messages(),
        all_count,
        max_width,
    );

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

        let entry_text = truncate_sidebar_entry(
            row_marker(is_selected && is_focused, is_active),
            label,
            count,
            max_width,
        );

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

    // Real terminal cursor on the highlighted row (row 1 is the separator
    // when there is room for it).
    if is_focused {
        let row = match app.sidebar_selected {
            0 => 0,
            n if inner.height > 2 => n + 1,
            n => n,
        };
        super::park_cursor(frame, inner, row);
    }
}

/// Leading marker of a sidebar row: `>` where the cursor is (sidebar
/// focused), `•` on the active filter. In text, so neither state depends on
/// color or boldness alone.
fn row_marker(is_cursor: bool, is_active: bool) -> char {
    if is_cursor {
        '>'
    } else if is_active {
        '\u{2022}'
    } else {
        ' '
    }
}

/// Format a sidebar entry as `"<marker>Label Name  (123)"`, exactly
/// `max_width` columns wide, truncating the label with `...` if needed.
///
/// Measured in terminal columns, so CJK and emoji labels line up.
fn truncate_sidebar_entry(marker: char, label: &str, count: usize, max_width: usize) -> String {
    let count_str = format!(" ({count})");
    let avail = max_width.saturating_sub(1 + count_str.width());
    let label_w = label.width();
    if label_w <= avail {
        return format!("{marker}{label}{}{count_str}", " ".repeat(avail - label_w));
    }
    if avail <= 3 {
        return format!("{marker}{label}");
    }
    let mut cut = String::new();
    let mut used = 0;
    for ch in label.chars() {
        let w = ch.width().unwrap_or(0);
        if used + w > avail - 3 {
            break;
        }
        cut.push(ch);
        used += w;
    }
    let pad = " ".repeat(avail - 3 - used);
    format!("{marker}{cut}...{pad}{count_str}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_sidebar_entry_ascii_fits() {
        // Short label is padded, not truncated; formatting unchanged.
        let out = truncate_sidebar_entry(' ', "Inbox", 12, 24);
        assert!(out.starts_with(" Inbox"));
        assert!(out.contains("(12)"));
        assert!(!out.contains("..."));
    }

    #[test]
    fn test_truncate_sidebar_entry_multibyte_label_no_panic() {
        // A non-ASCII label wider than the budget must truncate on a char
        // boundary instead of panicking mid-character (this runs every frame).
        let label = "Categoría-Ñoños-Español-中文分類";
        for max_width in [8usize, 10, 24] {
            let out = truncate_sidebar_entry(' ', label, 3, max_width);
            assert!(out.starts_with(' '));
            if max_width >= 10 {
                assert_eq!(out.width(), max_width, "{out:?}");
            }
        }
    }

    #[test]
    fn test_sidebar_markers_are_textual() {
        assert!(truncate_sidebar_entry(row_marker(true, true), "Inbox", 1, 20).starts_with('>'));
        assert!(
            truncate_sidebar_entry(row_marker(false, true), "Inbox", 1, 20).starts_with('\u{2022}')
        );
        assert!(truncate_sidebar_entry(row_marker(false, false), "Inbox", 1, 20).starts_with(' '));
    }
}
