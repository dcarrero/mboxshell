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
            widgets::mail_list::render(frame, app, content_area);
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

    // Status bar or search bar
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
}
