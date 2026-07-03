//! Mail view widget — displays the content of the selected message.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::i18n;
use crate::tui::app::{App, BodyMatch, PanelFocus};
use crate::tui::text::sanitize_line;
use crate::tui::theme::current_theme;

/// Render the message view panel.
pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = current_theme();

    let is_focused = app.focus == PanelFocus::MailView;
    let border_style = if is_focused {
        theme.border_focused
    } else {
        theme.border
    };

    let title = if app.show_raw {
        i18n::tui_message_raw()
    } else if app.show_full_headers {
        i18n::tui_message_headers()
    } else {
        i18n::tui_message_title()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title);

    let inner = block.inner(area);

    // While the in-body search prompt is open, reserve the top inner row for it
    // so it sits right above the body being searched (instead of the global
    // bottom bar). The body then scrolls within the remaining area.
    let (prompt_area, body_area) = if app.body_search_active {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(inner);
        (Some(rows[0]), rows[1])
    } else {
        (None, inner)
    };
    app.message_view_height = body_area.height as usize;

    let entry = match app.current_entry() {
        Some(e) => e,
        None => {
            frame.render_widget(block, area);
            if let Some(prompt_area) = prompt_area {
                super::body_search_bar::render(frame, app, prompt_area);
            }
            let empty = Paragraph::new(i18n::tui_no_message()).style(theme.message_body);
            frame.render_widget(empty, body_area);
            return;
        }
    };

    let mut lines: Vec<Line> = Vec::new();

    if app.show_raw {
        // Show raw message source
        if let Some(body) = &app.current_body {
            let raw = &body.raw_headers;
            for line in raw.lines() {
                lines.push(Line::from(Span::styled(
                    sanitize_line(line).into_owned(),
                    theme.message_body,
                )));
            }
            lines.push(Line::from(""));
            if let Some(text) = &body.text {
                for line in text.lines() {
                    lines.push(Line::from(Span::styled(
                        sanitize_line(line).into_owned(),
                        theme.message_body,
                    )));
                }
            }
        }
    } else {
        // Headers
        let header_fields = if app.show_full_headers {
            // Show all raw headers
            if let Some(body) = &app.current_body {
                for line in body.raw_headers.lines() {
                    let line = sanitize_line(line);
                    if let Some(colon_pos) = line.find(':') {
                        let label = &line[..colon_pos + 1];
                        let value = line[colon_pos + 1..].trim();
                        lines.push(Line::from(vec![
                            Span::styled(format!("{label} "), theme.message_header_label),
                            Span::styled(value.to_string(), theme.message_header_value),
                        ]));
                    } else {
                        lines.push(Line::from(Span::styled(
                            format!("  {line}"),
                            theme.message_header_value,
                        )));
                    }
                }
                false // Already rendered
            } else {
                true
            }
        } else {
            true
        };

        if header_fields {
            // Standard compact headers
            lines.push(Line::from(vec![
                Span::styled(i18n::tui_header_date(), theme.message_header_label),
                Span::styled(
                    entry.date.format("%a, %d %b %Y %H:%M:%S %z").to_string(),
                    theme.message_header_value,
                ),
            ]));

            lines.push(Line::from(vec![
                Span::styled(i18n::tui_header_from(), theme.message_header_label),
                Span::styled(
                    sanitize_line(&entry.from.display()).into_owned(),
                    theme.message_header_value,
                ),
            ]));

            if !entry.to.is_empty() {
                let to_str = entry
                    .to
                    .iter()
                    .map(|a| a.display())
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(Line::from(vec![
                    Span::styled(i18n::tui_header_to(), theme.message_header_label),
                    Span::styled(
                        sanitize_line(&to_str).into_owned(),
                        theme.message_header_value,
                    ),
                ]));
            }

            if !entry.cc.is_empty() {
                let cc_str = entry
                    .cc
                    .iter()
                    .map(|a| a.display())
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(Line::from(vec![
                    Span::styled(i18n::tui_header_cc(), theme.message_header_label),
                    Span::styled(
                        sanitize_line(&cc_str).into_owned(),
                        theme.message_header_value,
                    ),
                ]));
            }

            lines.push(Line::from(vec![
                Span::styled(i18n::tui_header_subject(), theme.message_header_label),
                Span::styled(
                    sanitize_line(&entry.subject).into_owned(),
                    theme.message_header_value,
                ),
            ]));
        }

        // Separator
        let sep_width = inner.width as usize;
        lines.push(Line::from(Span::styled(
            "\u{2500}".repeat(sep_width),
            theme.border,
        )));
        lines.push(Line::from(""));

        // Body text. Record where the body begins so in-body search can map a
        // match's body-relative line to an absolute scroll offset.
        app.body_line_start = lines.len();
        if let Some(body) = &app.current_body {
            if let Some(text) = &body.text {
                for (idx, line) in text.lines().enumerate() {
                    // Sanitize before styling. The in-body search sanitizes the
                    // same way, so match byte ranges line up with what is shown.
                    let line = sanitize_line(line);
                    // Highlight in-body search matches when present, otherwise
                    // fall back to plain URL detection.
                    let styled_line = style_body_line(
                        &line,
                        &theme,
                        &app.body_search_matches,
                        app.body_search_index,
                        idx,
                    );
                    lines.push(styled_line);
                }
            } else {
                lines.push(Line::from(Span::styled(
                    i18n::tui_no_text_content(),
                    theme.message_body,
                )));
            }

            // Attachments summary
            if !body.attachments.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!(
                        "[{}: {} file(s)]",
                        i18n::tui_attachments_count(),
                        body.attachments.len()
                    ),
                    theme.attachment,
                )));
                for att in &body.attachments {
                    let size = humansize::format_size(att.size, humansize::BINARY);
                    lines.push(Line::from(Span::styled(
                        format!("  @ {} ({})", att.filename, size),
                        theme.attachment,
                    )));
                }
            }
        }
    }

    // Scrolling is measured in *wrapped* rows — the unit ratatui actually
    // scrolls over once `Wrap` splits long lines. `line_count` reuses ratatui's
    // own word-wrap, so these counts match the rendered output exactly.
    let visible_height = body_area.height as usize;
    let body_width = body_area.width;

    // Bring the focused in-body match into view, if navigation requested it.
    // We measure the wrapped rows preceding the match's line and centre on it,
    // so the match lands inside the viewport regardless of earlier wrapping.
    if app.body_search_recenter {
        if let Some(m) = app.body_search_matches.get(app.body_search_index) {
            let target = (app.body_line_start + m.line).min(lines.len());
            let rows_before = Paragraph::new(lines[..target].to_vec())
                .wrap(Wrap { trim: false })
                .line_count(body_width);
            app.message_scroll_offset = rows_before.saturating_sub(visible_height / 2);
        }
        app.body_search_recenter = false;
    }

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    let total_wrapped = paragraph.line_count(body_width).max(1);
    let max_scroll = total_wrapped.saturating_sub(visible_height);
    let scroll = app.message_scroll_offset.min(max_scroll);

    // Build the scroll indicator for the bottom border (e.g. "[ ↕ 45% ]"),
    // appending an in-body search match counter when a search is active.
    let match_info = if app.body_search_matches.is_empty() {
        None
    } else {
        Some((app.body_search_index + 1, app.body_search_matches.len()))
    };
    let block = block
        .title_bottom(scroll_indicator(scroll, max_scroll, match_info, &theme).right_aligned());
    frame.render_widget(block, area);

    // In-body search prompt at the top of the panel (when open).
    if let Some(prompt_area) = prompt_area {
        super::body_search_bar::render(frame, app, prompt_area);
    }

    frame.render_widget(paragraph.scroll((scroll as u16, 0)), body_area);
}

/// Build the body scroll indicator shown in the bottom border.
///
/// Signals at a glance whether the message body is scrollable and where the
/// viewport sits: `[ All ]` when everything fits, `[ ↓ Top ]` at the start,
/// `[ ↑ Bot ]` at the end, and `[ ↕ NN% ]` in between. The percentage is an
/// approximation, since the offset counts unwrapped lines while the paragraph
/// scrolls over wrapped ones.
fn scroll_indicator<'a>(
    scroll: usize,
    max_scroll: usize,
    match_info: Option<(usize, usize)>,
    theme: &crate::tui::theme::Theme,
) -> Line<'a> {
    let scroll_label = if max_scroll == 0 {
        format!(" [ {} ] ", i18n::tui_scroll_all())
    } else if scroll == 0 {
        format!(" [ \u{2193} {} ] ", i18n::tui_scroll_top())
    } else if scroll >= max_scroll {
        format!(" [ \u{2191} {} ] ", i18n::tui_scroll_bot())
    } else {
        let percent = (scroll * 100) / max_scroll;
        format!(" [ \u{2195} {percent}% ] ")
    };

    let mut spans = Vec::new();
    if let Some((current, total)) = match_info {
        spans.push(Span::styled(
            format!(" [ {current}/{total} ] "),
            theme.search_highlight,
        ));
    }
    spans.push(Span::styled(scroll_label, theme.border));
    Line::from(spans)
}

/// Style a single body line for display.
///
/// When the in-body search has matches on this line, those matches are
/// highlighted (the focused one is emphasised) and URL colouring is skipped for
/// the line. Otherwise the line falls back to plain URL highlighting.
fn style_body_line<'a>(
    line: &str,
    theme: &crate::tui::theme::Theme,
    matches: &[BodyMatch],
    active_index: usize,
    line_idx: usize,
) -> Line<'a> {
    // Collect this line's matches in reading order, tagging the focused one.
    let hits: Vec<(usize, usize, bool)> = matches
        .iter()
        .enumerate()
        .filter(|(_, m)| m.line == line_idx)
        .map(|(global_idx, m)| (m.start, m.end, global_idx == active_index))
        .collect();

    if hits.is_empty() {
        return style_urls(line, theme);
    }

    let mut spans = Vec::new();
    let mut last_end = 0;
    for (start, end, is_active) in hits {
        if start > last_end {
            spans.push(Span::styled(
                line[last_end..start].to_string(),
                theme.message_body,
            ));
        }
        let style = if is_active {
            theme
                .search_highlight
                .add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            theme.search_highlight
        };
        spans.push(Span::styled(line[start..end].to_string(), style));
        last_end = end;
    }
    if last_end < line.len() {
        spans.push(Span::styled(
            line[last_end..].to_string(),
            theme.message_body,
        ));
    }
    Line::from(spans)
}

/// Style a single body line, highlighting URLs.
fn style_urls<'a>(line: &str, theme: &crate::tui::theme::Theme) -> Line<'a> {
    let mut spans = Vec::new();
    let mut last_end = 0;

    // Simple URL detection
    for (start, _) in line
        .match_indices("http://")
        .chain(line.match_indices("https://"))
    {
        if start > last_end {
            spans.push(Span::styled(
                line[last_end..start].to_string(),
                theme.message_body,
            ));
        }

        // Find end of URL (space, >, ), or end of line)
        let url_start = start;
        let rest = &line[url_start..];
        let url_end = rest
            .find(|c: char| c.is_whitespace() || c == '>' || c == ')' || c == '"')
            .unwrap_or(rest.len());
        let url = &line[url_start..url_start + url_end];

        spans.push(Span::styled(url.to_string(), theme.url));
        last_end = url_start + url_end;
    }

    if last_end < line.len() {
        spans.push(Span::styled(
            line[last_end..].to_string(),
            theme.message_body,
        ));
    }

    if spans.is_empty() {
        Line::from(Span::styled(line.to_string(), theme.message_body))
    } else {
        Line::from(spans)
    }
}

#[cfg(test)]
mod render_tests {
    use crate::model::mail::MailBody;
    use crate::tui::app::{App, LayoutMode, PanelFocus};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    /// Flatten each terminal row into a string for substring assertions.
    fn rendered_rows(term: &Terminal<TestBackend>) -> String {
        let buf = term.backend().buffer();
        let width = buf.area.width as usize;
        buf.content()
            .chunks(width)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Regression test for the in-body search auto-scroll (issue #12): on a body
    /// whose early lines wrap many times, navigating to a match near the end must
    /// bring it on-screen. The previous code scrolled by *unwrapped* line index,
    /// so the wrapped rows piled up and the match stayed off-screen.
    #[test]
    fn in_body_match_is_scrolled_into_view_despite_wrapping() {
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");

        // 40 long lines (each wraps into several rows) then a unique needle that
        // fits on one line so it stays contiguous in the rendered buffer.
        let long = "lorem ipsum dolor sit amet consectetur ".repeat(6);
        let mut text = String::new();
        for _ in 0..40 {
            text.push_str(&long);
            text.push('\n');
        }
        text.push_str("here is the UNIQUENEEDLE token\n");
        app.current_body = Some(MailBody {
            text: Some(text),
            html: None,
            raw_headers: String::new(),
            attachments: Vec::new(),
        });
        app.layout = LayoutMode::HorizontalSplit;
        app.focus = PanelFocus::MailView;
        app.body_search_query = "UNIQUENEEDLE".to_string();
        app.recompute_body_matches();
        assert_eq!(app.body_search_matches.len(), 1, "needle occurs once");
        assert!(app.body_search_recenter, "recompute requests a recentre");

        let mut term = Terminal::new(TestBackend::new(40, 24)).expect("terminal");
        term.draw(|f| crate::tui::ui::render(f, &mut app))
            .expect("draw");

        let visible = rendered_rows(&term);
        assert!(
            visible.contains("UNIQUENEEDLE"),
            "focused match must be scrolled into view, got:\n{visible}"
        );
    }

    /// Smoke test: the full UI renders in every layout plus the help overlay
    /// without panicking and produces a non-empty frame. Guards the ratatui
    /// render pipeline (notably across the 0.30 upgrade).
    #[test]
    fn full_ui_renders_across_layouts_and_popups() {
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");
        assert!(!app.entries.is_empty(), "fixture has messages");

        for layout in [
            LayoutMode::ListOnly,
            LayoutMode::HorizontalSplit,
            LayoutMode::VerticalSplit,
        ] {
            app.layout = layout;
            let mut term = Terminal::new(TestBackend::new(80, 24)).expect("terminal");
            term.draw(|f| crate::tui::ui::render(f, &mut app))
                .expect("draw");
            assert!(
                rendered_rows(&term)
                    .trim()
                    .chars()
                    .any(|c| !c.is_whitespace()),
                "a layout rendered an empty frame"
            );
        }

        // Help overlay on top of the list.
        app.show_help = true;
        let mut term = Terminal::new(TestBackend::new(80, 24)).expect("terminal");
        term.draw(|f| crate::tui::ui::render(f, &mut app))
            .expect("draw");
        assert!(rendered_rows(&term)
            .trim()
            .chars()
            .any(|c| !c.is_whitespace()));
    }
}
