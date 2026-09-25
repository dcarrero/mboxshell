//! Main render function that dispatches to widgets.

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Frame;

use super::app::{App, LayoutMode};
use super::widgets;

/// Render the entire TUI frame.
pub fn render(frame: &mut Frame, app: &mut App) {
    let size = frame.area();

    // Vertical layout: header (1) + content (flex) + status (1)
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header bar
            Constraint::Min(5),    // content
            Constraint::Length(1), // status bar or search bar
        ])
        .split(size);

    // Header bar
    widgets::header_bar::render(frame, app, vertical[0]);

    // Content area with optional sidebar
    let content_area = if app.show_sidebar && !app.all_labels.is_empty() {
        let h_split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(24), Constraint::Min(30)])
            .split(vertical[1]);
        widgets::sidebar::render(frame, app, h_split[0]);
        h_split[1]
    } else {
        vertical[1]
    };

    // Main content — depends on layout mode
    match app.layout {
        LayoutMode::ListOnly => {
            // In fullscreen mode, Tab/Enter toggles between list and message.
            // Show the message view when MailView has focus; otherwise show the list.
            if app.focus == super::app::PanelFocus::MailView {
                widgets::mail_view::render(frame, app, content_area);
            } else {
                widgets::mail_list::render(frame, app, content_area);
            }
        }
        LayoutMode::HorizontalSplit => {
            let split = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
                .split(content_area);
            widgets::mail_list::render(frame, app, split[0]);
            widgets::mail_view::render(frame, app, split[1]);
        }
        LayoutMode::VerticalSplit => {
            let split = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
                .split(content_area);
            widgets::mail_list::render(frame, app, split[0]);
            widgets::mail_view::render(frame, app, split[1]);
        }
    }

    // Bottom bar: global search prompt or status bar. The in-body search prompt
    // is drawn at the top of the message panel itself (see `mail_view`), next to
    // the body being searched, so it does not appear here.
    if app.search_active {
        widgets::search_bar::render(frame, app, vertical[2]);
    } else {
        widgets::status_bar::render(frame, app, vertical[2]);
    }

    // Popups (rendered on top of everything)
    if app.show_help {
        widgets::help_popup::render(frame, app);
    }
    if app.show_attachments {
        widgets::attachment_popup::render(frame, app);
    }
    if app.show_export {
        widgets::export_popup::render(frame, app);
    }
    if app.show_search_filter {
        widgets::search_popup::render(frame, app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::PathBuf;

    #[test]
    fn renders_every_screen_at_small_and_large_sizes() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple.mbox");
        let screens: [fn(&mut App); 5] = [
            |_| {},
            |a| a.show_help = true,
            |a| a.search_active = true,
            |a| a.show_search_filter = true,
            |a| a.show_export = true,
        ];
        for (w, h) in [(20, 8), (34, 12), (40, 12), (80, 24), (120, 40)] {
            for open in screens {
                let mut app = App::new(path.clone(), true).expect("open fixture");
                open(&mut app);
                let mut term = Terminal::new(TestBackend::new(w, h)).expect("terminal");
                term.draw(|f| render(f, &mut app)).expect("draw");
                // Scroll the help to the end: the marker and clamping must hold.
                app.help_scroll = usize::MAX / 2;
                term.draw(|f| render(f, &mut app)).expect("draw");
                if app.show_help {
                    assert!(app.help_scroll <= app.help_max_scroll);
                }
            }
        }
    }
}
