//! Mail view widget — displays the content of the selected message.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::i18n;
use crate::tui::app::{App, PanelFocus};
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
    app.message_view_height = inner.height as usize;
    frame.render_widget(block, area);

    let entry = match app.current_entry() {
        Some(e) => e,
        None => {
            let empty = Paragraph::new(i18n::tui_no_message()).style(theme.message_body);
            frame.render_widget(empty, inner);
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
                    line.to_string(),
                    theme.message_body,
                )));
            }
            lines.push(Line::from(""));
            if let Some(text) = &body.text {
                for line in text.lines() {
                    lines.push(Line::from(Span::styled(
                        line.to_string(),
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
                Span::styled(entry.from.display(), theme.message_header_value),
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
                    Span::styled(to_str, theme.message_header_value),
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
                    Span::styled(cc_str, theme.message_header_value),
                ]));
            }

            lines.push(Line::from(vec![
                Span::styled(i18n::tui_header_subject(), theme.message_header_label),
                Span::styled(entry.subject.clone(), theme.message_header_value),
            ]));
        }

        // Separator
        let sep_width = inner.width as usize;
        lines.push(Line::from(Span::styled(
            "\u{2500}".repeat(sep_width),
            theme.border,
        )));
        lines.push(Line::from(""));

        // Body text
        if let Some(body) = &app.current_body {
            if let Some(text) = &body.text {
                for line in text.lines() {
                    // Detect URLs and highlight them
                    let styled_line = style_body_line(line, &theme);
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

    // Apply scroll offset
    let total_lines = lines.len();
    let visible_height = inner.height as usize;
    let max_scroll = total_lines.saturating_sub(visible_height);
    let scroll = app.message_scroll_offset.min(max_scroll);

    let paragraph = Paragraph::new(lines)
        .scroll((scroll as u16, 0))
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, inner);
}

/// Style a single body line, highlighting URLs.
fn style_body_line<'a>(line: &str, theme: &crate::tui::theme::Theme) -> Line<'a> {
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
