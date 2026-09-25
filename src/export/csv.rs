//! Export message summaries to CSV.
//!
//! Output is UTF-8 with BOM for Excel compatibility.

use std::io::Write;
use std::path::Path;

use crate::model::mail::MailEntry;

/// Export a list of entries to a CSV file.
///
/// Columns: Date, From, To, CC, Subject, Size, Has_Attachments, Labels, Message_ID
///
/// If `include_snippet` is true and `snippets` is provided, a "Snippet" column
/// is added with the first 200 chars of the body text.
pub fn export_csv(
    entries: &[&MailEntry],
    output_path: &Path,
    snippets: Option<&[String]>,
) -> anyhow::Result<()> {
    let mut file = std::fs::File::create(output_path)?;

    // UTF-8 BOM for Excel
    file.write_all(&[0xEF, 0xBB, 0xBF])?;

    // Header row
    let mut header = "Date,From,To,CC,Subject,Size,Has_Attachments,Labels,Message_ID".to_string();
    if snippets.is_some() {
        header.push_str(",Snippet");
    }
    writeln!(file, "{header}")?;

    // Data rows
    for (i, entry) in entries.iter().enumerate() {
        let date = entry.date.format("%Y-%m-%d %H:%M:%S").to_string();
        let from = format!("{} <{}>", entry.from.display_name, entry.from.address);
        let to_str = join_guarded(entry.to.iter().map(format_address));
        let cc_str = join_guarded(entry.cc.iter().map(format_address));
        let labels = join_guarded(entry.labels.iter().cloned());

        let mut row = format!(
            "{},{},{},{},{},{},{},{},{}",
            csv_escape(&date),
            csv_escape(&from),
            csv_escape(&to_str),
            csv_escape(&cc_str),
            csv_escape(&entry.subject),
            entry.length,
            entry.has_attachments,
            csv_escape(&labels),
            csv_escape(&entry.message_id),
        );

        if let Some(snips) = snippets {
            let snippet = snips.get(i).map(|s| s.as_str()).unwrap_or("");
            row.push(',');
            row.push_str(&csv_escape(snippet));
        }

        writeln!(file, "{row}")?;
    }

    Ok(())
}

/// `Display Name <address>`, as it appears in the From/To/CC columns.
fn format_address(a: &crate::model::address::EmailAddress) -> String {
    format!("{} <{}>", a.display_name, a.address)
}

/// Join list items with `"; "`, formula-guarding each item first.
///
/// A spreadsheet opened with `;` as its list separator (Excel in es-ES and
/// many other locales) may split such a cell, and every fragment after a `;`
/// must be as inert as the start of the cell — including a `;` inside an item
/// (a display name such as `x;=cmd` is attacker-controlled).
fn join_guarded(items: impl Iterator<Item = String>) -> String {
    items
        .map(|item| {
            item.split(';')
                .map(formula_guard)
                .collect::<Vec<_>>()
                .join(";")
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Prefix `'` to a value that a spreadsheet would run as a formula: one whose
/// first character — ignoring leading spaces, which spreadsheets skip — is
/// `=`, `+`, `-`, `@`, tab or CR (CSV injection).
fn formula_guard(value: &str) -> String {
    let needs_guard = matches!(
        value.trim_start_matches(' ').chars().next(),
        Some('=' | '+' | '-' | '@' | '\t' | '\r')
    );
    if needs_guard {
        format!("'{value}")
    } else {
        value.to_string()
    }
}

/// Escape a value for CSV (RFC 4180).
///
/// The value is formula-guarded (see [`formula_guard`]), then wrapped in
/// double quotes if it contains a comma, a semicolon, a quote or a line break.
/// Semicolons are quoted too because spreadsheets in locales whose decimal
/// separator is the comma (Excel in es-ES, de-DE, fr-FR…) use `;` as the field
/// separator, and an unquoted one would split the cell.
fn csv_escape(value: &str) -> String {
    let guarded = formula_guard(value);
    if guarded.contains([',', ';', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csv_escape_simple() {
        assert_eq!(csv_escape("hello"), "hello");
    }

    #[test]
    fn test_csv_escape_comma() {
        assert_eq!(csv_escape("hello, world"), "\"hello, world\"");
    }

    #[test]
    fn test_csv_escape_quotes() {
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn test_csv_escape_newline() {
        assert_eq!(csv_escape("line1\nline2"), "\"line1\nline2\"");
    }

    #[test]
    fn test_csv_escape_formula_injection() {
        assert_eq!(csv_escape("=cmd|'/c calc'!A1"), "'=cmd|'/c calc'!A1");
        assert_eq!(csv_escape("+1234"), "'+1234");
        assert_eq!(csv_escape("-2+3"), "'-2+3");
        assert_eq!(csv_escape("@SUM(A1)"), "'@SUM(A1)");
    }

    #[test]
    fn test_csv_escape_formula_injection_with_comma() {
        // Guard prefix and RFC 4180 quoting must compose.
        assert_eq!(csv_escape("=1,2"), "\"'=1,2\"");
    }

    #[test]
    fn test_csv_escape_inner_equals_not_guarded() {
        assert_eq!(csv_escape("a=b"), "a=b");
    }

    #[test]
    fn test_csv_escape_quotes_semicolon() {
        assert_eq!(csv_escape("a; b"), "\"a; b\"");
        assert_eq!(csv_escape("=1;2"), "\"'=1;2\"");
    }

    #[test]
    fn test_csv_escape_guards_after_leading_spaces() {
        assert_eq!(csv_escape("  =1+1"), "'  =1+1");
    }

    #[test]
    fn test_join_guarded_guards_every_fragment() {
        let joined = join_guarded(
            [
                "alice;=cmd <a@x>",
                "=HYPERLINK(\"http://evil\") <b@x>",
                "+1 <c@x>",
                "@x <d@x>",
            ]
            .into_iter()
            .map(String::from),
        );
        // Split the way a `;`-separated spreadsheet would: no fragment may
        // start (after spaces) with a formula trigger.
        for fragment in joined.split(';') {
            let first = fragment.trim_start().chars().next();
            assert!(
                !matches!(first, Some('=' | '+' | '-' | '@')),
                "fragment {fragment:?} would run as a formula"
            );
        }
        // And the whole cell is quoted, so neither `,` nor `;` splits it.
        let cell = csv_escape(&joined);
        assert!(cell.starts_with('"') && cell.ends_with('"'), "{cell}");
    }

    #[test]
    fn test_export_csv_row_is_safe_with_comma_and_semicolon_separators() {
        use crate::model::address::EmailAddress;
        use chrono::TimeZone;
        let addr = |name: &str, address: &str| EmailAddress {
            display_name: name.to_string(),
            address: address.to_string(),
        };
        let entry = MailEntry {
            offset: 0,
            length: 10,
            date: chrono::Utc.with_ymd_and_hms(2024, 1, 4, 9, 0, 0).unwrap(),
            from: addr("-cmd", "e@x"),
            to: vec![addr("Ana", "a@x"), addr("=2+5", "b@x")],
            cc: vec![addr("@SUM(A1)", "c@x")],
            subject: "Hi; there, you".to_string(),
            message_id: "<m@x>".to_string(),
            in_reply_to: None,
            references: vec![],
            has_attachments: false,
            content_type: "text/plain".to_string(),
            text_size: 1,
            labels: vec!["Inbox".to_string(), "=evil".to_string()],
            sequence: 0,
            thread_id: None,
        };
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.csv");
        export_csv(&[&entry], &out, None).unwrap();
        let text = String::from_utf8(std::fs::read(&out).unwrap()).unwrap();
        let row = text.lines().nth(1).unwrap();
        assert_eq!(
            row,
            "2024-01-04 09:00:00,'-cmd <e@x>,\"Ana <a@x>; '=2+5 <b@x>\",'@SUM(A1) <c@x>,\
             \"Hi; there, you\",10,false,\"Inbox; '=evil\",<m@x>"
        );
    }
}
