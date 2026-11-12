//! Sanitization of untrusted message text for terminal rendering.

use std::borrow::Cow;
use unicode_width::UnicodeWidthChar;

/// Tab stop width used when expanding tabs in message text.
const TAB_STOP: usize = 8;

/// Make one line of untrusted message text safe for ratatui rendering.
///
/// ratatui requires text to be free of control characters: a raw tab,
/// escape or stray carriage return is written to the terminal as-is and
/// moves the real cursor in ways the buffer diff cannot track, leaving
/// stale cells from previous frames on screen (issue #17). Tabs are
/// expanded to [`TAB_STOP`]-column tab stops (tracking display width, so
/// wide characters keep columns aligned); every other control character
/// is replaced with U+FFFD (`�`) to stay visible without being executed.
///
/// Bidirectional control characters (overrides, embeddings, isolates and
/// marks) are removed: they are invisible, and a file name like
/// `invoice<U+202E>fdp.exe` is displayed as `invoiceexe.pdf` by a terminal that
/// applies them.
///
/// Returns the input unchanged — without allocating — when it is already
/// clean, which is the overwhelmingly common case.
pub fn sanitize_line(line: &str) -> Cow<'_, str> {
    if !line.chars().any(|c| c.is_control() || is_bidi_control(c)) {
        return Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len() + TAB_STOP);
    let mut col = 0usize;
    for ch in line.chars() {
        if ch == '\t' {
            let pad = TAB_STOP - (col % TAB_STOP);
            out.extend(std::iter::repeat_n(' ', pad));
            col += pad;
        } else if ch.is_control() {
            out.push('\u{FFFD}');
            col += 1;
        } else if is_bidi_control(ch) {
            // Dropped, not replaced: they have no width and legitimate
            // right-to-left text is full of them.
        } else {
            out.push(ch);
            col += ch.width().unwrap_or(0);
        }
    }
    Cow::Owned(out)
}

/// Unicode bidirectional formatting characters: LRM/RLM/ALM, the embedding
/// and override controls (U+202A-U+202E) and the isolates (U+2066-U+2069).
fn is_bidi_control(c: char) -> bool {
    matches!(
        c,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_line_is_borrowed() {
        let line = "plain text, no controls";
        assert!(matches!(sanitize_line(line), Cow::Borrowed(_)));
    }

    #[test]
    fn test_tabs_expand_to_tab_stops() {
        assert_eq!(sanitize_line("\tx"), "        x");
        assert_eq!(sanitize_line("ab\tx"), "ab      x");
        assert_eq!(sanitize_line("12345678\tx"), "12345678        x");
        // Wide characters count as two columns toward the next stop
        assert_eq!(sanitize_line("漢\tx"), "漢      x");
    }

    #[test]
    fn test_control_chars_replaced() {
        assert_eq!(sanitize_line("a\u{1b}[31mred"), "a\u{FFFD}[31mred"); // ESC
        assert_eq!(sanitize_line("mid\rline"), "mid\u{FFFD}line"); // stray CR
        assert_eq!(sanitize_line("a\u{0c}b"), "a\u{FFFD}b"); // form feed
        assert_eq!(sanitize_line("a\u{0}b"), "a\u{FFFD}b"); // NUL
    }

    #[test]
    fn test_bidi_controls_are_removed() {
        // Right-to-left override: shown as `invoiceexe.pdf` if left in.
        assert_eq!(sanitize_line("invoice\u{202E}fdp.exe"), "invoicefdp.exe");
        assert_eq!(sanitize_line("a\u{2066}b\u{2069}c\u{200F}"), "abc");
        // The text itself, right-to-left or not, is untouched.
        assert_eq!(sanitize_line("שלום\u{200F} עולם"), "שלום עולם");
    }

    #[test]
    fn test_unicode_text_untouched() {
        let line = "Reunión — café ☕ 漢字";
        assert_eq!(sanitize_line(line), line);
    }
}
