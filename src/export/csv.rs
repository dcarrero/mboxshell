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
        let to_str = entry
            .to
            .iter()
            .map(|a| format!("{} <{}>", a.display_name, a.address))
            .collect::<Vec<_>>()
            .join("; ");
        let cc_str = entry
            .cc
            .iter()
            .map(|a| format!("{} <{}>", a.display_name, a.address))
            .collect::<Vec<_>>()
            .join("; ");
        let labels = entry.labels.join("; ");

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

/// Escape a value for CSV (RFC 4180).
///
/// Wraps in double quotes if the value contains commas, quotes, or newlines.
/// Values starting with `=`, `+`, `-`, `@`, tab or CR get a leading `'` so
/// spreadsheet apps (Excel, LibreOffice) treat them as text instead of
/// executing them as formulas (CSV injection).
fn csv_escape(value: &str) -> String {
    let needs_guard = matches!(
        value.chars().next(),
        Some('=' | '+' | '-' | '@' | '\t' | '\r')
    );
    let guarded = if needs_guard {
        format!("'{value}")
    } else {
        value.to_string()
    };
    if guarded.contains(',')
        || guarded.contains('"')
        || guarded.contains('\n')
        || guarded.contains('\r')
    {
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
}
