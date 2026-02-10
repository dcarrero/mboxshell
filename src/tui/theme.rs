//! Color theme definitions for the TUI.

use ratatui::style::{Color, Modifier, Style};

/// A complete color theme for the TUI.
pub struct Theme {
    pub header_bar: Style,
    pub status_bar: Style,
    pub list_selected: Style,
    pub list_marked: Style,
    pub list_header: Style,
    pub list_normal: Style,
    pub sidebar: Style,
    pub sidebar_selected: Style,
    pub message_header_label: Style,
    pub message_header_value: Style,
    pub message_body: Style,
    pub url: Style,
    pub search_highlight: Style,
    pub attachment: Style,
    pub border: Style,
    pub popup: Style,
    pub popup_title: Style,
    pub search_prompt: Style,
}

impl Theme {
    /// Dark theme (default).
    pub fn dark() -> Self {
        Self {
            header_bar: Style::default()
                .fg(Color::Rgb(200, 200, 220))
                .bg(Color::Rgb(30, 30, 46)),
            status_bar: Style::default()
                .fg(Color::Rgb(150, 150, 170))
                .bg(Color::Rgb(30, 30, 46)),
            list_selected: Style::default()
                .fg(Color::White)
                .bg(Color::Rgb(60, 60, 100)),
            list_marked: Style::default().fg(Color::Yellow),
            list_header: Style::default()
                .fg(Color::Rgb(180, 180, 200))
                .bg(Color::Rgb(40, 40, 60))
                .add_modifier(Modifier::BOLD),
            list_normal: Style::default().fg(Color::Rgb(200, 200, 220)),
            sidebar: Style::default().fg(Color::Rgb(180, 180, 200)),
            sidebar_selected: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            message_header_label: Style::default()
                .fg(Color::Rgb(130, 170, 255))
                .add_modifier(Modifier::BOLD),
            message_header_value: Style::default().fg(Color::Rgb(220, 220, 230)),
            message_body: Style::default().fg(Color::Rgb(220, 220, 230)),
            url: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::UNDERLINED),
            search_highlight: Style::default().fg(Color::Black).bg(Color::Yellow),
            attachment: Style::default().fg(Color::Green),
            border: Style::default().fg(Color::Rgb(80, 80, 100)),
            popup: Style::default()
                .fg(Color::Rgb(220, 220, 230))
                .bg(Color::Rgb(20, 20, 35)),
            popup_title: Style::default()
                .fg(Color::Rgb(130, 170, 255))
                .add_modifier(Modifier::BOLD),
            search_prompt: Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        }
    }
}

/// Return the active theme.
pub fn current_theme() -> Theme {
    Theme::dark()
}
