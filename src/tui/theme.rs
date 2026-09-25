//! Color theme definitions for the TUI.
//!
//! Three themes, picked with `theme` in `config.toml` or `MBOXSHELL_THEME`:
//!
//! - `dark` (default): fixed RGB colors for a dark terminal background.
//! - `light`: fixed RGB colors on its own light background (painted over
//!   the whole screen, so the terminal's background does not matter), every
//!   text pair at WCAG AA (4.5:1) or better.
//! - `terminal`: no colors at all. Text keeps the terminal's own foreground
//!   and background and state is shown with bold, underline and reverse
//!   video, so it follows whatever palette (and contrast) the user already
//!   tuned. `NO_COLOR` (<https://no-color.org>) always selects it.

use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};

/// A complete color theme for the TUI.
pub struct Theme {
    /// Painted over the whole screen before anything else. Only the light
    /// theme sets it: it brings its own background instead of assuming the
    /// terminal's is light, so it reads the same on a dark terminal.
    pub base: Style,
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
    pub border_focused: Style,
    pub popup: Style,
    pub popup_title: Style,
    pub help_section: Style,
    pub help_dim: Style,
    pub search_prompt: Style,
}

impl Theme {
    /// Dark theme (default).
    pub fn dark() -> Self {
        Self {
            base: Style::default(),
            header_bar: Style::default()
                .fg(Color::Rgb(200, 200, 220))
                .bg(Color::Rgb(30, 30, 46)),
            status_bar: Style::default()
                .fg(Color::Rgb(150, 150, 170))
                .bg(Color::Rgb(30, 30, 46)),
            // The background alone is 2:1 against the terminal's; bold keeps
            // the row distinct without relying on color, next to the `>`
            // marker the list draws.
            list_selected: Style::default()
                .fg(Color::White)
                .bg(Color::Rgb(60, 60, 100))
                .add_modifier(Modifier::BOLD),
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
            message_header_value: Style::default().fg(Color::Rgb(235, 235, 245)),
            message_body: Style::default().fg(Color::Rgb(235, 235, 245)),
            url: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::UNDERLINED),
            search_highlight: Style::default().fg(Color::Black).bg(Color::Yellow),
            attachment: Style::default().fg(Color::Green),
            border: Style::default().fg(Color::Rgb(120, 120, 145)),
            border_focused: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            popup: Style::default()
                .fg(Color::Rgb(220, 220, 230))
                .bg(Color::Rgb(20, 20, 35)),
            popup_title: Style::default()
                .fg(Color::Rgb(130, 170, 255))
                .add_modifier(Modifier::BOLD),
            help_section: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            // 6.4:1 on the status bar; the old (100,100,120) was 2.8:1 and
            // carries real content (search syntax, match counter).
            help_dim: Style::default().fg(Color::Rgb(160, 160, 180)),
            search_prompt: Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        }
    }
}

impl Theme {
    /// Light theme, for terminals with a light background.
    pub fn light() -> Self {
        let bar = Color::Rgb(222, 222, 234);
        let accent = Color::Rgb(0, 85, 160);
        let label = Color::Rgb(0, 65, 165);
        Self {
            base: Style::default()
                .fg(Color::Rgb(20, 20, 30))
                .bg(Color::Rgb(250, 250, 252)),
            header_bar: Style::default().fg(Color::Rgb(25, 25, 40)).bg(bar),
            status_bar: Style::default().fg(Color::Rgb(55, 55, 75)).bg(bar),
            list_selected: Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(190, 205, 250))
                .add_modifier(Modifier::BOLD),
            list_marked: Style::default().fg(Color::Rgb(140, 80, 0)),
            list_header: Style::default()
                .fg(Color::Rgb(30, 30, 50))
                .bg(Color::Rgb(208, 208, 222))
                .add_modifier(Modifier::BOLD),
            list_normal: Style::default().fg(Color::Rgb(35, 35, 50)),
            sidebar: Style::default().fg(Color::Rgb(45, 45, 65)),
            sidebar_selected: Style::default().fg(accent).add_modifier(Modifier::BOLD),
            message_header_label: Style::default().fg(label).add_modifier(Modifier::BOLD),
            message_header_value: Style::default().fg(Color::Rgb(20, 20, 30)),
            message_body: Style::default().fg(Color::Rgb(20, 20, 30)),
            url: Style::default()
                .fg(accent)
                .add_modifier(Modifier::UNDERLINED),
            search_highlight: Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(255, 214, 0)),
            attachment: Style::default().fg(Color::Rgb(0, 105, 40)),
            border: Style::default().fg(Color::Rgb(125, 125, 145)),
            border_focused: Style::default().fg(accent).add_modifier(Modifier::BOLD),
            popup: Style::default()
                .fg(Color::Rgb(20, 20, 30))
                .bg(Color::Rgb(244, 244, 250)),
            popup_title: Style::default().fg(label).add_modifier(Modifier::BOLD),
            help_section: Style::default().fg(accent).add_modifier(Modifier::BOLD),
            help_dim: Style::default().fg(Color::Rgb(85, 85, 105)),
            search_prompt: Style::default()
                .fg(Color::Rgb(140, 75, 0))
                .add_modifier(Modifier::BOLD),
        }
    }

    /// Colorless theme: the terminal's own colors plus text attributes.
    pub fn terminal() -> Self {
        let plain = Style::default();
        let bold = plain.add_modifier(Modifier::BOLD);
        let reversed = plain.add_modifier(Modifier::REVERSED);
        Self {
            base: plain,
            header_bar: reversed,
            status_bar: reversed,
            list_selected: reversed.add_modifier(Modifier::BOLD),
            list_marked: bold,
            list_header: bold.add_modifier(Modifier::UNDERLINED),
            list_normal: plain,
            sidebar: plain,
            sidebar_selected: bold,
            message_header_label: bold,
            message_header_value: plain,
            message_body: plain,
            url: plain.add_modifier(Modifier::UNDERLINED),
            search_highlight: reversed,
            attachment: plain,
            border: plain,
            border_focused: bold,
            popup: plain,
            popup_title: bold,
            help_section: bold.add_modifier(Modifier::UNDERLINED),
            help_dim: plain,
            search_prompt: bold,
        }
    }
}

/// Which theme the TUI draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    /// Fixed colors for a dark background (default).
    Dark,
    /// Fixed colors for a light background.
    Light,
    /// No colors: the terminal's palette plus bold/underline/reverse.
    Terminal,
}

impl ThemeKind {
    /// Parse a theme name as written in the config (`dark`, `light`,
    /// `terminal`; `mono`/`none` are accepted for `terminal`).
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            "terminal" | "mono" | "none" | "no-color" => Some(Self::Terminal),
            _ => None,
        }
    }

    /// Resolve the theme from the environment and the configured name.
    ///
    /// `NO_COLOR` set to anything non-empty wins, as the convention asks;
    /// then `MBOXSHELL_THEME`, then the config value. An unknown name falls
    /// back to `dark` rather than failing to start.
    pub fn resolve(configured: &str) -> Self {
        Self::resolve_with(
            std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()),
            std::env::var("MBOXSHELL_THEME").ok().as_deref(),
            configured,
        )
    }

    fn resolve_with(no_color: bool, env_theme: Option<&str>, configured: &str) -> Self {
        if no_color {
            return Self::Terminal;
        }
        env_theme
            .and_then(Self::from_name)
            .or_else(|| Self::from_name(configured))
            .unwrap_or(Self::Dark)
    }
}

static ACTIVE_THEME: OnceLock<ThemeKind> = OnceLock::new();

/// Choose the theme for this run. Only the first call has any effect; the
/// TUI makes it once at startup, before the first frame.
pub fn set_theme(kind: ThemeKind) {
    let _ = ACTIVE_THEME.set(kind);
}

/// Return the active theme.
pub fn current_theme() -> Theme {
    match ACTIVE_THEME.get().copied().unwrap_or(ThemeKind::Dark) {
        ThemeKind::Dark => Theme::dark(),
        ThemeKind::Light => Theme::light(),
        ThemeKind::Terminal => Theme::terminal(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_resolution_order() {
        use ThemeKind::*;
        assert_eq!(ThemeKind::resolve_with(false, None, "dark"), Dark);
        assert_eq!(ThemeKind::resolve_with(false, None, "Light"), Light);
        assert_eq!(
            ThemeKind::resolve_with(false, Some("terminal"), "light"),
            Terminal
        );
        // An unreadable env value defers to the config, not to the default.
        assert_eq!(
            ThemeKind::resolve_with(false, Some("bogus"), "light"),
            Light
        );
        assert_eq!(ThemeKind::resolve_with(false, None, "bogus"), Dark);
        // NO_COLOR beats everything.
        assert_eq!(
            ThemeKind::resolve_with(true, Some("light"), "light"),
            Terminal
        );
    }

    #[test]
    fn test_light_theme_brings_its_own_background() {
        // On a dark terminal the light theme drew dark text on black.
        assert!(Theme::light().base.bg.is_some());
        assert_eq!(Theme::dark().base, Style::default());
    }

    #[test]
    fn test_terminal_theme_sets_no_colors() {
        let t = Theme::terminal();
        for style in [
            t.header_bar,
            t.list_selected,
            t.message_body,
            t.search_highlight,
            t.popup,
        ] {
            assert_eq!(style.fg, None);
            assert_eq!(style.bg, None);
        }
    }
}
