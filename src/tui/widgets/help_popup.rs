//! Help popup showing keyboard shortcuts in multi-column layout.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

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
pub fn render(frame: &mut Frame, _app: &App) {
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
    let lines = build_lines(cols, col_width, sep_width, &theme);

    // Size popup to fit content: lines + 2 (borders) + 1 (bottom padding)
    let content_height = lines.len() as u16 + 1; // +1 small bottom padding
    let popup_height = (content_height + 2).min(screen.height.saturating_sub(2)); // +2 borders

    let area = centered_rect_exact(popup_width, popup_height, screen);

    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.popup_title)
        .title(i18n::tui_help_title())
        .style(theme.popup);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Build all the help content lines.
fn build_lines<'a>(
    cols: usize,
    col_width: usize,
    sep_width: usize,
    theme: &crate::tui::theme::Theme,
) -> Vec<Line<'a>> {
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
                key: "F",
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
        "    from: to: subject: body: label: date: size: has:attachment",
        theme.help_dim,
    )));
    lines.push(Line::from(Span::styled(
        format!("    {}", i18n::tui_help_search_history()),
        theme.help_dim,
    )));
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
                key: "L",
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

    lines
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
                let desc_truncated = if s.desc.len() > desc_avail {
                    format!("{}.", &s.desc[..desc_avail.saturating_sub(1)])
                } else {
                    s.desc.to_string()
                };
                let padding = col_width
                    .saturating_sub(key_w + 1 + desc_truncated.len())
                    .max(1);

                spans.push(Span::styled(padded_key, theme.search_prompt));
                spans.push(Span::styled(format!(" {desc_truncated}"), theme.popup));
                spans.push(Span::raw(" ".repeat(padding)));
            }
        }

        lines.push(Line::from(spans));
    }
}

/// Calculate a centered rectangle with exact pixel dimensions, clamped to screen.
fn centered_rect_exact(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}
