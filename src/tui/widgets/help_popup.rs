//! Help popup showing keyboard shortcuts in multi-column layout.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

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
    let area = centered_rect(78, 85, frame.area());

    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.popup_title)
        .title(" Help ")
        .style(theme.popup);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let total_width = inner.width as usize;
    // Decide columns: 3 if wide enough, else 2
    let cols = if total_width >= 90 {
        3
    } else if total_width >= 56 {
        2
    } else {
        1
    };
    let col_width = total_width / cols;

    let mut lines: Vec<Line> = Vec::new();

    // ── App header ─────────────────────────────────
    lines.push(Line::from(vec![
        Span::styled("  mboxShell", theme.popup_title),
        Span::styled(format!("  v{VERSION}"), theme.help_dim),
    ]));
    lines.push(Line::from(Span::styled(
        "  Fast terminal viewer for MBOX files",
        theme.help_dim,
    )));
    lines.push(Line::from(""));

    let sep_width = total_width.saturating_sub(2);

    // ── Navigation ─────────────────────────────────
    add_section_header(&mut lines, "Navigation", &theme, sep_width);
    add_shortcuts_columns(
        &mut lines,
        &[
            Shortcut {
                key: "j / k",
                desc: "Next / prev",
            },
            Shortcut {
                key: "g / G",
                desc: "First / last",
            },
            Shortcut {
                key: "PgDn/Up",
                desc: "Page scroll",
            },
            Shortcut {
                key: "Enter",
                desc: "Open message",
            },
            Shortcut {
                key: "Tab",
                desc: "Cycle panel",
            },
            Shortcut {
                key: "Esc",
                desc: "Back / close",
            },
        ],
        cols,
        col_width,
        &theme,
    );
    lines.push(Line::from(""));

    // ── Message & Export ───────────────────────────
    add_section_header(&mut lines, "Message & Export", &theme, sep_width);
    add_shortcuts_columns(
        &mut lines,
        &[
            Shortcut {
                key: "h",
                desc: "Full headers",
            },
            Shortcut {
                key: "r",
                desc: "Raw source",
            },
            Shortcut {
                key: "e",
                desc: "Export menu",
            },
            Shortcut {
                key: "a",
                desc: "Attachments",
            },
        ],
        cols,
        col_width,
        &theme,
    );
    lines.push(Line::from(""));

    // ── List Actions ──────────────────────────────
    add_section_header(&mut lines, "List Actions", &theme, sep_width);
    add_shortcuts_columns(
        &mut lines,
        &[
            Shortcut {
                key: "Space",
                desc: "Mark / unmark",
            },
            Shortcut {
                key: "*",
                desc: "Mark all",
            },
            Shortcut {
                key: "s",
                desc: "Cycle sort col",
            },
            Shortcut {
                key: "S",
                desc: "Sort direction",
            },
            Shortcut {
                key: "t",
                desc: "Thread view",
            },
        ],
        cols,
        col_width,
        &theme,
    );
    lines.push(Line::from(""));

    // ── Search ────────────────────────────────────
    add_section_header(&mut lines, "Search", &theme, sep_width);
    add_shortcuts_columns(
        &mut lines,
        &[
            Shortcut {
                key: "/",
                desc: "Search bar",
            },
            Shortcut {
                key: "n / N",
                desc: "Next / prev result",
            },
        ],
        cols,
        col_width,
        &theme,
    );
    lines.push(Line::from(Span::styled(
        "    from: to: subject: body: label: date: size: has:attachment",
        theme.help_dim,
    )));
    lines.push(Line::from(""));

    // ── Layout & General ──────────────────────────
    add_section_header(&mut lines, "Layout & General", &theme, sep_width);
    add_shortcuts_columns(
        &mut lines,
        &[
            Shortcut {
                key: "1/2/3",
                desc: "Layout mode",
            },
            Shortcut {
                key: "L",
                desc: "Labels sidebar",
            },
            Shortcut {
                key: "?",
                desc: "This help",
            },
            Shortcut {
                key: "q",
                desc: "Quit",
            },
            Shortcut {
                key: "Ctrl-C",
                desc: "Force quit",
            },
        ],
        cols,
        col_width,
        &theme,
    );
    lines.push(Line::from(""));

    // ── Footer ────────────────────────────────────
    let sep = "\u{2500}".repeat(sep_width);
    lines.push(Line::from(Span::styled(format!("  {sep}"), theme.help_dim)));
    lines.push(Line::from(Span::styled(
        "  MIT License - David Carrero Fernandez-Baillo",
        theme.help_dim,
    )));
    lines.push(Line::from(Span::styled(
        "  https://github.com/dcarrero/mboxshell",
        theme.help_dim,
    )));

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
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
    // Key portion width inside each column
    let key_w: usize = 8;

    for row_start in (0..shortcuts.len()).step_by(cols) {
        let mut spans: Vec<Span<'static>> = Vec::new();
        spans.push(Span::raw("  ")); // left margin

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

/// Calculate a centered rectangle with given percentage of width and height.
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let width = area.width * percent_x / 100;
    let height = area.height * percent_y / 100;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}
