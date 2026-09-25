//! Search filter popup for building queries visually.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::i18n;
use crate::tui::app::{App, SearchFilterField, SIZE_OPTIONS};
use crate::tui::theme::current_theme;

/// Render the search filter popup centered on screen.
pub fn render(frame: &mut Frame, app: &App) {
    let theme = current_theme();
    let screen = frame.area();

    let has_labels = !app.all_labels.is_empty();
    let row_count: u16 = if has_labels { 10 } else { 9 };
    // rows + title border (1) + footer (2) + borders (2) + padding (1)
    let popup_height = (row_count + 6).min(screen.height.saturating_sub(2));
    let popup_width = (screen.width * 60 / 100)
        .max(50)
        .min(screen.width.saturating_sub(4));

    let area = centered_rect_exact(popup_width, popup_height, screen);

    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.popup_title)
        .title(i18n::tui_search_filters_title())
        .style(theme.popup);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = build_lines(app, &theme, has_labels, inner.width);

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);

    // Real terminal cursor on the focused field, for screen readers and
    // magnifiers (the fields used to show a drawn `_` instead).
    let (row, col) = focus_cursor(app, has_labels, inner.width);
    if row < inner.height && col < inner.width {
        frame.set_cursor_position((inner.x + col, inner.y + row));
    }
}

/// Width of the label column shared by every row.
const LABEL_W: usize = 15;

/// Placeholder shown in an empty, unfocused date field.
const DATE_PLACEHOLDER: &str = "___________";

/// `(row, column)` inside the popup where the focused field takes input.
fn focus_cursor(app: &App, has_labels: bool, inner_width: u16) -> (u16, u16) {
    let value_w = (inner_width as usize).saturating_sub(LABEL_W + 2);
    let value_col = 2 + LABEL_W;
    let typed = |v: &str| truncate_str(v, value_w.saturating_sub(1)).width();
    let date_from_shown = if app.filter_date_from.is_empty() {
        DATE_PLACEHOLDER.width()
    } else {
        app.filter_date_from.width()
    };
    let (row, offset) = match app.search_filter_focus {
        SearchFilterField::Text => (0, typed(&app.filter_text)),
        SearchFilterField::From => (1, typed(&app.filter_from)),
        SearchFilterField::To => (2, typed(&app.filter_to)),
        SearchFilterField::Subject => (3, typed(&app.filter_subject)),
        SearchFilterField::DateFrom => (4, app.filter_date_from.width()),
        SearchFilterField::DateTo => (
            4,
            date_from_shown
                + format!("   {} ", i18n::tui_filter_date_to()).width()
                + app.filter_date_to.width(),
        ),
        SearchFilterField::Size => (5, 0),
        SearchFilterField::HasAttachment => (6, 1),
        SearchFilterField::Label => (7, 0),
        SearchFilterField::WithinResults => (if has_labels { 8 } else { 7 }, 1),
    };
    (row, (value_col + offset) as u16)
}

/// Build all content lines for the search filter popup.
fn build_lines(
    app: &App,
    theme: &crate::tui::theme::Theme,
    has_labels: bool,
    inner_width: u16,
) -> Vec<Line<'static>> {
    let label_w = LABEL_W;
    let value_w = (inner_width as usize).saturating_sub(label_w + 2);
    let focus = app.search_filter_focus;

    let size_label = SIZE_OPTIONS
        .get(app.filter_size_selected)
        .map(|&(name, _)| name)
        .unwrap_or(i18n::tui_filter_any());

    let check_mark = if app.filter_has_attachment {
        "[x]"
    } else {
        "[ ]"
    };

    let mut lines = vec![
        build_text_row(
            i18n::tui_filter_text(),
            &app.filter_text,
            label_w,
            value_w,
            focus == SearchFilterField::Text,
            theme,
        ),
        build_text_row(
            i18n::tui_filter_from(),
            &app.filter_from,
            label_w,
            value_w,
            focus == SearchFilterField::From,
            theme,
        ),
        build_text_row(
            i18n::tui_filter_to(),
            &app.filter_to,
            label_w,
            value_w,
            focus == SearchFilterField::To,
            theme,
        ),
        build_text_row(
            i18n::tui_filter_subject(),
            &app.filter_subject,
            label_w,
            value_w,
            focus == SearchFilterField::Subject,
            theme,
        ),
        build_date_row(
            &app.filter_date_from,
            &app.filter_date_to,
            label_w,
            value_w,
            focus,
            theme,
        ),
        build_selector_row(
            i18n::tui_filter_size(),
            size_label,
            label_w,
            focus == SearchFilterField::Size,
            theme,
        ),
        build_checkbox_row(
            i18n::tui_filter_attachment(),
            check_mark,
            i18n::tui_filter_has_attachment(),
            label_w,
            focus == SearchFilterField::HasAttachment,
            theme,
        ),
    ];

    if has_labels {
        let label_display = if app.filter_label_selected == 0 {
            i18n::tui_filter_any()
        } else {
            app.all_labels
                .get(app.filter_label_selected - 1)
                .map(|s| s.as_str())
                .unwrap_or(i18n::tui_filter_any())
        };
        lines.push(build_selector_row(
            i18n::tui_filter_label(),
            label_display,
            label_w,
            focus == SearchFilterField::Label,
            theme,
        ));
    }

    let within_check = if app.filter_within_results {
        "[x]"
    } else {
        "[ ]"
    };
    lines.push(build_checkbox_row(
        i18n::tui_filter_within_label(),
        within_check,
        i18n::tui_filter_within_results(),
        label_w,
        focus == SearchFilterField::WithinResults,
        theme,
    ));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("  {}", i18n::tui_filter_footer()),
        theme.help_dim,
    )));

    lines
}

/// Build a text input row.
fn build_text_row<'a>(
    label: &str,
    value: &str,
    label_w: usize,
    value_w: usize,
    focused: bool,
    theme: &crate::tui::theme::Theme,
) -> Line<'a> {
    let padded_label = format!("  {label:<width$}", width = label_w);
    let display_value = if focused {
        // One column is kept free for the terminal cursor.
        truncate_str(value, value_w.saturating_sub(1)).to_string()
    } else if value.is_empty() {
        String::new()
    } else {
        truncate_str(value, value_w).to_string()
    };

    let value_style = if focused {
        theme.list_selected
    } else {
        theme.popup
    };

    Line::from(vec![
        Span::styled(padded_label, theme.message_header_label),
        Span::styled(display_value, value_style),
    ])
}

/// Build a date range row with two fields on one line.
fn build_date_row<'a>(
    date_from: &str,
    date_to: &str,
    label_w: usize,
    _value_w: usize,
    focus: SearchFilterField,
    theme: &crate::tui::theme::Theme,
) -> Line<'a> {
    let padded_label = format!(
        "  {:<width$}",
        i18n::tui_filter_date_from(),
        width = label_w
    );

    let from_focused = focus == SearchFilterField::DateFrom;
    let to_focused = focus == SearchFilterField::DateTo;

    let from_display = if from_focused {
        date_from.to_string()
    } else if date_from.is_empty() {
        DATE_PLACEHOLDER.to_string()
    } else {
        date_from.to_string()
    };

    let to_display = if to_focused {
        date_to.to_string()
    } else if date_to.is_empty() {
        DATE_PLACEHOLDER.to_string()
    } else {
        date_to.to_string()
    };

    let from_style = if from_focused {
        theme.list_selected
    } else {
        theme.popup
    };
    let to_style = if to_focused {
        theme.list_selected
    } else {
        theme.popup
    };

    Line::from(vec![
        Span::styled(padded_label, theme.message_header_label),
        Span::styled(from_display, from_style),
        Span::styled(
            format!("   {} ", i18n::tui_filter_date_to()),
            theme.message_header_label,
        ),
        Span::styled(to_display, to_style),
    ])
}

/// Build a selector row (e.g., Size, Label) with `< value >` when focused.
fn build_selector_row<'a>(
    label: &str,
    value: &str,
    label_w: usize,
    focused: bool,
    theme: &crate::tui::theme::Theme,
) -> Line<'a> {
    let padded_label = format!("  {label:<width$}", width = label_w);
    let display_value = if focused {
        format!("< {value} >")
    } else {
        value.to_string()
    };

    let value_style = if focused {
        theme.list_selected
    } else {
        theme.popup
    };

    Line::from(vec![
        Span::styled(padded_label, theme.message_header_label),
        Span::styled(display_value, value_style),
    ])
}

/// Build a checkbox row.
fn build_checkbox_row<'a>(
    label: &str,
    check: &str,
    description: &str,
    label_w: usize,
    focused: bool,
    theme: &crate::tui::theme::Theme,
) -> Line<'a> {
    let padded_label = format!("  {label:<width$}", width = label_w);

    let value_style = if focused {
        theme.list_selected
    } else {
        theme.popup
    };

    Line::from(vec![
        Span::styled(padded_label, theme.message_header_label),
        Span::styled(format!("{check} {description}"), value_style),
    ])
}

/// Truncate a string to at most `max_len` characters.
fn truncate_str(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        // Find a safe char boundary
        let mut end = max_len;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

/// Calculate a centered rectangle with exact dimensions, clamped to screen.
fn centered_rect_exact(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}
