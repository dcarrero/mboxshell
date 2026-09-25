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
