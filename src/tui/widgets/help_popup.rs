//! Help popup showing keyboard shortcuts in multi-column layout.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::i18n;
use crate::tui::app::App;
use crate::tui::theme::current_theme;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A single shortcut entry.
struct Shortcut {
    key: &'static str,
    desc: &'static str,
}

/// Render the help popup centered on screen with multi-column shortcuts.
pub fn render(frame: &mut Frame, app: &mut App) {
    let theme = current_theme();
    let screen = frame.area();

    // First pass: determine width and column count using a preliminary width
    let popup_width = (screen.width * 78 / 100).min(screen.width.saturating_sub(4));
    let inner_width = popup_width.saturating_sub(2) as usize; // borders

    let cols = if inner_width >= 90 {
        3
    } else if inner_width >= 56 {
        2
    } else {
        1
    };
    let col_width = inner_width / cols;
    let sep_width = inner_width.saturating_sub(2);

    // Build all lines
    let (lines, syntax_line) = build_lines(cols, col_width, sep_width, &theme);

    // Size popup to fit content: lines + 2 (borders) + 1 (bottom padding)
    let content_height = lines.len() as u16 + 1; // +1 small bottom padding
    let popup_height = (content_height + 2).min(screen.height.saturating_sub(2)); // +2 borders

    let area = centered_rect_exact(popup_width, popup_height, screen);

    frame.render_widget(Clear, area);

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.popup_title)
        .title(i18n::tui_help_title())
        .style(theme.popup);

    // Scroll when the shortcuts do not fit, with a "first-last/total" marker
    // in the bottom border so it is obvious there is more below.
    let visible = area.height.saturating_sub(2) as usize;
    let total = lines.len();
    app.help_max_scroll = total.saturating_sub(visible);
    if app.help_jump_to_syntax {
        app.help_jump_to_syntax = false;
        app.help_scroll = syntax_line;
    }
    app.help_scroll = app.help_scroll.min(app.help_max_scroll);
    if app.help_max_scroll > 0 {
        let first = app.help_scroll + 1;
        let last = (app.help_scroll + visible).min(total);
        block = block.title_bottom(Line::from(format!(" {first}-{last}/{total} ")).right_aligned());
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let paragraph = Paragraph::new(lines).scroll((app.help_scroll as u16, 0));
    frame.render_widget(paragraph, inner);
    super::park_cursor(frame, inner, 0);
}

/// Build all the help content lines, plus the index of the search syntax
/// header so F1 can scroll straight to it.
fn build_lines<'a>(
    cols: usize,
    col_width: usize,
    sep_width: usize,
    theme: &crate::tui::theme::Theme,
) -> (Vec<Line<'a>>, usize) {
    let mut lines: Vec<Line> = Vec::new();

    // ── App header ─────────────────────────────────
    lines.push(Line::from(vec![
        Span::styled(format!("  {}", i18n::app_name()), theme.popup_title),
        Span::styled(format!("  v{VERSION}"), theme.help_dim),
    ]));
    lines.push(Line::from(Span::styled(
        format!("  {}", i18n::tui_help_description()),
        theme.help_dim,
    )));
    lines.push(Line::from(""));

    // ── Navigation ─────────────────────────────────
    add_section_header(&mut lines, i18n::tui_help_navigation(), theme, sep_width);
    add_shortcuts_columns(
        &mut lines,
        &[
            Shortcut {
                key: "j / k",
                desc: i18n::tui_help_next_prev(),
            },
            Shortcut {
                key: "g / G",
                desc: i18n::tui_help_first_last(),
            },
            Shortcut {
                key: "PgDn/Up",
                desc: i18n::tui_help_page_scroll(),
            },
            Shortcut {
                key: "Shift+\u{2191}\u{2193}",
                desc: i18n::tui_help_scroll_body(),
            },
            Shortcut {
                key: "Enter",
                desc: i18n::tui_help_open_message(),
            },
            Shortcut {
                key: "Tab",
                desc: i18n::tui_help_cycle_panel(),
            },
            Shortcut {
                key: "Esc",
                desc: i18n::tui_help_back_close(),
            },
        ],
        cols,
        col_width,
        theme,
    );
    lines.push(Line::from(""));

    // ── Message & Export ───────────────────────────
    add_section_header(
        &mut lines,
        i18n::tui_help_message_export(),
        theme,
        sep_width,
    );
    add_shortcuts_columns(
        &mut lines,
        &[
            Shortcut {
                key: "h",
                desc: i18n::tui_help_full_headers(),
            },
            Shortcut {
                key: "r",
                desc: i18n::tui_help_raw_source(),
            },
            Shortcut {
                key: "e",
                desc: i18n::tui_help_export_menu(),
            },
            Shortcut {
                key: "a",
                desc: i18n::tui_help_attachments(),
            },
            Shortcut {
                key: "/  n/N",
                desc: i18n::tui_help_find_in_body(),
            },
        ],
        cols,
        col_width,
        theme,
    );
    lines.push(Line::from(""));

    // ── List Actions ──────────────────────────────
    add_section_header(&mut lines, i18n::tui_help_list_actions(), theme, sep_width);
    add_shortcuts_columns(
        &mut lines,
        &[
            Shortcut {
                key: "Space",
                desc: i18n::tui_help_mark_unmark(),
            },
            Shortcut {
                key: "*",
                desc: i18n::tui_help_mark_all(),
            },
            Shortcut {
                key: "s",
                desc: i18n::tui_help_cycle_sort(),
            },
            Shortcut {
                key: "S",
                desc: i18n::tui_help_sort_direction(),
            },
            Shortcut {
                key: "t",
                desc: i18n::tui_help_thread_view(),
            },
        ],
        cols,
        col_width,
        theme,
    );
    lines.push(Line::from(""));

    // ── Search ────────────────────────────────────
    add_section_header(&mut lines, i18n::tui_help_search(), theme, sep_width);
    add_shortcuts_columns(
        &mut lines,
        &[
            Shortcut {
                key: "/",
                desc: i18n::tui_help_search_bar(),
            },
            Shortcut {
                key: "f",
                desc: i18n::tui_help_filter_popup(),
            },
            Shortcut {
                key: "n / N",
                desc: i18n::tui_help_next_prev_result(),
            },
        ],
        cols,
        col_width,
        theme,
    );
    lines.push(Line::from(Span::styled(
        format!("    {}", i18n::tui_help_search_history()),
        theme.help_dim,
    )));
    lines.push(Line::from(Span::styled(
        format!("    {}", i18n::tui_help_search_syntax_key()),
        theme.help_dim,
    )));
    lines.push(Line::from(""));

    // ── Search syntax ─────────────────────────────
    let syntax_line = lines.len();
    add_section_header(&mut lines, i18n::search_syntax_title(), theme, sep_width);
    add_syntax_table(&mut lines, sep_width, theme);
    lines.push(Line::from(""));

    // ── Layout & General ──────────────────────────
    add_section_header(
        &mut lines,
        i18n::tui_help_layout_general(),
        theme,
        sep_width,
    );
    add_shortcuts_columns(
        &mut lines,
        &[
            Shortcut {
                key: "1/2/3",
                desc: i18n::tui_help_layout_mode(),
            },
            Shortcut {
                key: "l",
                desc: i18n::tui_help_labels_sidebar(),
            },
            Shortcut {
                key: "?",
                desc: i18n::tui_help_this_help(),
            },
            Shortcut {
                key: "q",
                desc: i18n::tui_help_quit(),
            },
            Shortcut {
                key: "Ctrl-C",
                desc: i18n::tui_help_force_quit(),
            },
        ],
        cols,
        col_width,
        theme,
    );
    lines.push(Line::from(""));

    // ── Footer ────────────────────────────────────
    let sep = "\u{2500}".repeat(sep_width);
    lines.push(Line::from(Span::styled(format!("  {sep}"), theme.help_dim)));
    lines.push(Line::from(Span::styled(
        "  MIT License - David Carrero Fernandez-Baillo - carrero.es",
        theme.help_dim,
    )));
    lines.push(Line::from(Span::styled(
        "  https://github.com/dcarrero/mboxshell",
        theme.help_dim,
    )));

    (lines, syntax_line)
}

/// One row per search operator, then the notes, all wrapped to the popup
/// width rather than cut: this is the only place in the app that spells the
/// syntax out, so a truncated description would hide the very detail
/// (a date format, a unit) someone opened it for.
///
/// A single column: the operators are wider than a shortcut key. On a
/// narrow terminal each description goes under its operator.
fn add_syntax_table(
    lines: &mut Vec<Line<'static>>,
    width: usize,
    theme: &crate::tui::theme::Theme,
) {
    let syntax = i18n::search_syntax();
    let op_w = syntax.iter().map(|(op, _)| op.width()).max().unwrap_or(0);
    // "    " + operator + "  " + description
    let side_by_side = 4 + op_w + 2 + 20 <= width;

    for (op, desc) in syntax {
        if side_by_side {
            let indent = " ".repeat(4 + op_w + 2);
            let rows = wrap_words(desc, width.saturating_sub(indent.len()));
            for (i, row) in rows.into_iter().enumerate() {
                let lead = if i == 0 {
                    Span::styled(format!("    {op:<op_w$}  "), theme.search_prompt)
                } else {
                    Span::raw(indent.clone())
                };
                lines.push(Line::from(vec![lead, Span::styled(row, theme.popup)]));
            }
        } else {
            lines.push(Line::from(Span::styled(
                format!("    {op}"),
                theme.search_prompt,
            )));
            for row in wrap_words(desc, width.saturating_sub(6)) {
                lines.push(Line::from(Span::styled(
                    format!("      {row}"),
                    theme.popup,
                )));
            }
        }
    }

    for row in wrap_words(i18n::search_syntax_notes(), width.saturating_sub(4)) {
        lines.push(Line::from(Span::styled(
            format!("    {row}"),
            theme.help_dim,
        )));
    }
}

/// Greedy word wrap measured in terminal columns. A word longer than the
/// width (a URL) gets a row of its own rather than being split.
fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let mut rows = Vec::new();
    let mut row = String::new();
    for word in text.split_whitespace() {
        if !row.is_empty() && row.width() + 1 + word.width() > width {
            rows.push(std::mem::take(&mut row));
        }
        if !row.is_empty() {
            row.push(' ');
        }
        row.push_str(word);
    }
    if !row.is_empty() {
        rows.push(row);
    }
    rows
}

/// Add a section header with a trailing separator line.
fn add_section_header(
    lines: &mut Vec<Line<'static>>,
    title: &str,
    theme: &crate::tui::theme::Theme,
    width: usize,
) {
    let title_len = title.len() + 4;
    let remaining = width.saturating_sub(title_len);
    let sep = "\u{2500}".repeat(remaining);
    lines.push(Line::from(vec![
        Span::styled(format!("  {title} "), theme.help_section),
        Span::styled(sep, theme.help_dim),
    ]));
}

/// Lay out shortcuts in N columns per row.
fn add_shortcuts_columns(
    lines: &mut Vec<Line<'static>>,
    shortcuts: &[Shortcut],
    cols: usize,
    col_width: usize,
    theme: &crate::tui::theme::Theme,
) {
    let key_w: usize = 8;

    for row_start in (0..shortcuts.len()).step_by(cols) {
        let mut spans: Vec<Span<'static>> = Vec::new();
        spans.push(Span::raw("  "));

        for c in 0..cols {
            let idx = row_start + c;
            if idx < shortcuts.len() {
                let s = &shortcuts[idx];
                let padded_key = format!("{:>width$}", s.key, width = key_w);
                let desc_avail = col_width.saturating_sub(key_w + 3);
                let desc_truncated = fit_to_width(s.desc, desc_avail);
                let padding = col_width
                    .saturating_sub(key_w + 1 + desc_truncated.width())
                    .max(1);

                spans.push(Span::styled(padded_key, theme.search_prompt));
                spans.push(Span::styled(format!(" {desc_truncated}"), theme.popup));
                spans.push(Span::raw(" ".repeat(padding)));
            }
        }

        lines.push(Line::from(spans));
    }
}

/// Fit `s` into `max_width` terminal columns, ending in `.` when cut.
///
/// Measured in columns, never bytes: slicing by byte length split the
/// Spanish descriptions ("página", "último") mid-character and panicked on
/// narrow terminals.
fn fit_to_width(s: &str, max_width: usize) -> String {
    if s.width() <= max_width {
        return s.to_string();
    }
    let budget = max_width.saturating_sub(1);
    let mut out = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let w = ch.width().unwrap_or(0);
        if used + w > budget {
            break;
        }
        out.push(ch);
        used += w;
    }
    if max_width > 0 {
        out.push('.');
    }
    out
}

/// Calculate a centered rectangle with exact pixel dimensions, clamped to screen.
fn centered_rect_exact(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_words_respects_width() {
        let rows = wrap_words("one two three four five", 9);
        assert_eq!(rows, ["one two", "three", "four five"]);
        // An over-long word stays whole on its own row.
        assert_eq!(
            wrap_words("a https://example.com b", 5),
            ["a", "https://example.com", "b"]
        );
        assert!(wrap_words("", 10).is_empty());
    }

    #[test]
    fn test_syntax_table_lists_every_field_operator() {
        let theme = crate::tui::theme::current_theme();
        let (lines, syntax_line) = build_lines(3, 30, 90, &theme);
        let text: String = lines[syntax_line..]
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        for op in [
            "from:",
            "to:",
            "cc:",
            "subject:",
            "body:",
            "filename:",
            "label:",
            "id:",
            "has:attachment",
            "date:",
            "after:",
            "before:",
            "size:",
            "OR",
        ] {
            assert!(text.contains(op), "{op} missing from the help");
        }
    }

    #[test]
    fn test_fit_to_width_never_splits_a_character() {
        // Every cut point through a multi-byte description must be safe.
        for width in 0..20 {
            let out = fit_to_width("Avance de página", width);
            assert!(out.width() <= width.max(1), "width {width}: {out:?}");
        }
        assert_eq!(fit_to_width("Primero / último", 12), "Primero / ú.");
        assert_eq!(fit_to_width("Salir", 10), "Salir");
    }
}
