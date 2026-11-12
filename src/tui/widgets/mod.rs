//! TUI widgets for rendering different UI panels.

pub mod attachment_popup;
pub mod body_search_bar;
pub mod export_popup;
pub mod header_bar;
pub mod help_popup;
pub mod mail_list;
pub mod mail_view;
pub mod search_bar;
pub mod search_popup;
pub mod sidebar;
pub mod status_bar;

use ratatui::layout::Rect;
use ratatui::Frame;

/// Put the terminal's real cursor at the start of `row` inside `area`.
///
/// Screen readers, braille displays and magnifiers track the cursor, so each
/// popup parks it on its selected line; out-of-range rows are ignored.
pub(crate) fn park_cursor(frame: &mut Frame, area: Rect, row: usize) {
    if area.width > 0 && row < area.height as usize {
        frame.set_cursor_position((area.x, area.y + row as u16));
    }
}

/// `percent` % of `value`, without overflowing `u16`.
///
/// `width * 78 / 100` in `u16` overflows past 840 columns: a panic in debug
/// builds and a popup of nonsensical width in release ones.
pub(crate) fn percent_of(value: u16, percent: u16) -> u16 {
    u16::try_from(u32::from(value) * u32::from(percent) / 100).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::percent_of;

    #[test]
    fn test_percent_of_does_not_overflow_on_huge_terminals() {
        assert_eq!(percent_of(100, 78), 78);
        assert_eq!(percent_of(1000, 78), 780);
        assert_eq!(percent_of(u16::MAX, 60), 39321);
        assert_eq!(percent_of(0, 60), 0);
    }
}
