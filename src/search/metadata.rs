//! Fast in-memory metadata search over the message index.
//!
//! Complexity: O(n) where n = number of messages.
//! For 1M messages this should complete in < 200ms.

use chrono::Datelike;

use crate::model::mail::MailEntry;

use super::query::{DateFilter, SearchField, SearchOperator, SearchQuery, SearchTerm, SizeFilter};

/// Search the index metadata and return matching entry indices.
///
/// Applies date/size/attachment filters first (cheapest), then text terms.
/// Uses short-circuit evaluation for AND queries.
///
/// `All` (free-text) terms are matched against metadata only (subject, from,
/// to). To let them also match the message body, use
/// [`search_metadata_candidates`] followed by a full-text pass.
pub fn search_metadata(entries: &[MailEntry], query: &SearchQuery) -> Vec<usize> {
    entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry_matches(entry, query, false))
        .map(|(i, _)| i)
        .collect()
}

/// Like [`search_metadata`], but for AND queries it does **not** require
/// `All` (free-text "Text") terms to match metadata — those are deferred to
/// the full-text pass in [`super::fulltext`] so a Text term can also match the
/// message body. Field-specific terms (`subject:`, `from:`, …) and the
/// date/size/attachment filters still apply.
///
/// For OR queries this behaves identically to [`search_metadata`] (deferring
/// `All` terms would incorrectly drop entries that match only via metadata).
pub fn search_metadata_candidates(entries: &[MailEntry], query: &SearchQuery) -> Vec<usize> {
    let defer_all = !query.is_or;
    entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry_matches(entry, query, defer_all))
        .map(|(i, _)| i)
        .collect()
}

/// Check whether a single entry matches the query.
///
/// When `defer_all` is set, `All` (free-text) terms are ignored here — the
/// caller is expected to verify them against the body in the full-text pass.
fn entry_matches(entry: &MailEntry, query: &SearchQuery, defer_all: bool) -> bool {
    // 1. Date filter (cheapest to check)
    if let Some(ref df) = query.date_filter {
        if !matches_date(entry, df) {
            return false;
        }
    }

    // 2. Size filter
    if let Some(ref sf) = query.size_filter {
        if !matches_size(entry, sf) {
            return false;
        }
    }

    // 3. Attachment filter
    if let Some(want_att) = query.has_attachment {
        if entry.has_attachments != want_att {
            return false;
        }
    }

    // 4. Text terms (skip body:/filename: terms — those need fulltext).
    //    When `defer_all` is set, also skip free-text `All` terms so they can
    //    be matched against the body in the full-text pass.
    let text_terms: Vec<&SearchTerm> = query
        .terms
        .iter()
        .filter(|t| t.field != SearchField::Body && t.field != SearchField::Filename)
        .filter(|t| !(defer_all && t.field == SearchField::All))
        .collect();

    if text_terms.is_empty() {
        return true;
    }

    if query.is_or {
        // OR: any term must match
        text_terms
            .iter()
            .any(|term| term_matches_entry(entry, term))
    } else {
        // AND: all terms must match
        text_terms
            .iter()
            .all(|term| term_matches_entry(entry, term))
    }
}

/// Check if a text term matches an entry's metadata.
fn term_matches_entry(entry: &MailEntry, term: &SearchTerm) -> bool {
    let raw_match = match term.field {
        SearchField::All => all_matches_metadata(entry, &term.operator),
        SearchField::From => {
            matches_text(&entry.from.address, &term.operator)
                || matches_text(&entry.from.display_name, &term.operator)
        }
        SearchField::To => entry.to.iter().any(|a| {
            matches_text(&a.address, &term.operator)
                || matches_text(&a.display_name, &term.operator)
        }),
        SearchField::Cc => entry.cc.iter().any(|a| {
            matches_text(&a.address, &term.operator)
                || matches_text(&a.display_name, &term.operator)
        }),
        SearchField::Subject => matches_text(&entry.subject, &term.operator),
        SearchField::Label => entry.labels.iter().any(|l| matches_text(l, &term.operator)),
        SearchField::MessageId => matches_text(&entry.message_id, &term.operator),
        // Body and Filename are handled by fulltext search, not metadata
        SearchField::Body | SearchField::Filename => true,
    };

    if term.negated {
        !raw_match
    } else {
        raw_match
    }
}

/// Whether a free-text (`All`) operator matches an entry's metadata, i.e. its
/// subject, sender, or any recipient (address or display name).
///
/// Shared with [`super::fulltext`] so the full-text pass can apply the same
/// "search everywhere" semantics to `All` terms (metadata **or** body).
pub(crate) fn all_matches_metadata(entry: &MailEntry, op: &SearchOperator) -> bool {
    matches_text(&entry.subject, op)
        || matches_text(&entry.from.address, op)
        || matches_text(&entry.from.display_name, op)
        || entry
            .to
            .iter()
            .any(|a| matches_text(&a.address, op) || matches_text(&a.display_name, op))
}

/// Case-insensitive text matching.
///
/// Both `Contains` and `Exact` use substring matching. The distinction is
/// purely lexical: `Exact` originates from a quoted phrase (so the entire
/// phrase, spaces included, is treated as a single needle), while
/// `Contains` is a single bareword. This mirrors the semantics of the
/// fulltext search and matches what users expect from search engines.
pub(crate) fn matches_text(haystack: &str, op: &SearchOperator) -> bool {
    let haystack_lower = haystack.to_lowercase();
    let needle = match op {
        SearchOperator::Contains(s) | SearchOperator::Exact(s) => s,
    };
    haystack_lower.contains(needle)
}

/// Check if entry's date matches the date filter.
fn matches_date(entry: &MailEntry, filter: &DateFilter) -> bool {
    let date = entry.date.date_naive();
    match filter {
        DateFilter::Exact(d) => date == *d,
        DateFilter::Range(start, end) => date >= *start && date <= *end,
        DateFilter::Before(d) => date < *d,
        DateFilter::After(d) => date > *d,
        DateFilter::Month(year, month) => date.year() == *year && date.month() == *month,
        DateFilter::Year(year) => date.year() == *year,
    }
}

/// Check if entry's size matches the size filter.
fn matches_size(entry: &MailEntry, filter: &SizeFilter) -> bool {
    match filter {
        SizeFilter::GreaterThan(threshold) => entry.length > *threshold,
        SizeFilter::LessThan(threshold) => entry.length < *threshold,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::address::EmailAddress;
    use crate::search::query::parse_query;
    use chrono::{TimeZone, Utc};

    fn make_entry(from: &str, subject: &str, date_str: &str) -> MailEntry {
        let date = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
            .map(|d| Utc.from_utc_datetime(&d.and_hms_opt(10, 0, 0).unwrap()))
            .unwrap_or(Utc::now());
        MailEntry {
            offset: 0,
            length: 1000,
            date,
            from: EmailAddress {
                display_name: String::new(),
                address: from.to_string(),
            },
            to: vec![EmailAddress {
                display_name: String::new(),
                address: "recipient@example.com".to_string(),
            }],
            cc: vec![],
            subject: subject.to_string(),
            message_id: format!("<msg-{subject}@example.com>"),
            in_reply_to: None,
            references: vec![],
            has_attachments: false,
            content_type: "text/plain".to_string(),
            text_size: 500,
            labels: vec![],
            sequence: 0,
        }
    }

    #[test]
    fn test_simple_search() {
        let entries = vec![
            make_entry("alice@example.com", "Budget Report", "2024-01-15"),
            make_entry("bob@example.com", "Meeting Notes", "2024-02-10"),
            make_entry("alice@example.com", "Re: Budget Report", "2024-01-20"),
        ];
        let q = parse_query("budget");
        let results = search_metadata(&entries, &q);
        assert_eq!(results, vec![0, 2]);
    }

    #[test]
    fn test_from_search() {
        let entries = vec![
            make_entry("alice@example.com", "Hello", "2024-01-01"),
            make_entry("bob@example.com", "World", "2024-01-02"),
        ];
        let q = parse_query("from:alice");
        let results = search_metadata(&entries, &q);
        assert_eq!(results, vec![0]);
    }

    #[test]
    fn test_negated_search() {
        let entries = vec![
            make_entry("alice@example.com", "Important", "2024-01-01"),
            make_entry("bob@example.com", "Spam stuff", "2024-01-02"),
            make_entry("carol@example.com", "Normal", "2024-01-03"),
        ];
        let q = parse_query("-subject:spam");
        let results = search_metadata(&entries, &q);
        assert_eq!(results, vec![0, 2]);
    }

    #[test]
    fn test_date_filter() {
        let entries = vec![
            make_entry("a@x.com", "Old", "2023-06-15"),
            make_entry("b@x.com", "New", "2024-03-10"),
            make_entry("c@x.com", "Recent", "2024-06-20"),
        ];
        let q = parse_query("date:2024-01..2024-06");
        let results = search_metadata(&entries, &q);
        assert_eq!(results, vec![1, 2]);
    }

    #[test]
    fn test_has_attachment_filter() {
        let mut entries = vec![
            make_entry("a@x.com", "No att", "2024-01-01"),
            make_entry("b@x.com", "Has att", "2024-01-02"),
        ];
        entries[1].has_attachments = true;

        let q = parse_query("has:attachment");
        let results = search_metadata(&entries, &q);
        assert_eq!(results, vec![1]);
    }

    #[test]
    fn test_combined_filters() {
        let entries = vec![
            make_entry("alice@x.com", "Budget Q1", "2024-01-15"),
            make_entry("alice@x.com", "Budget Q2", "2024-04-15"),
            make_entry("bob@x.com", "Budget Q1", "2024-01-20"),
        ];
        let q = parse_query("from:alice subject:budget date:2024-01");
        let results = search_metadata(&entries, &q);
        assert_eq!(results, vec![0]);
    }

    #[test]
    fn test_or_search() {
        let entries = vec![
            make_entry("alice@x.com", "Hello", "2024-01-01"),
            make_entry("bob@x.com", "World", "2024-01-02"),
            make_entry("carol@x.com", "Other", "2024-01-03"),
        ];
        let q = parse_query("from:alice OR from:bob");
        let results = search_metadata(&entries, &q);
        assert_eq!(results, vec![0, 1]);
    }

    #[test]
    fn test_quoted_phrase_is_substring_in_metadata() {
        // Regression for issue #4: quoted phrases must match by substring
        // in metadata, mirroring fulltext behaviour. Previously `Exact`
        // required full-string equality, so a quoted multi-word value
        // produced by the Search Filters popup never matched anything.
        let entries = vec![
            make_entry("alice@x.com", "Monthly Report Q1", "2024-01-15"),
            make_entry("bob@x.com", "Weekly Update", "2024-01-20"),
        ];
        let q = parse_query("subject:\"monthly report\"");
        let results = search_metadata(&entries, &q);
        assert_eq!(results, vec![0]);
    }

    #[test]
    fn test_combined_text_and_multiword_subject() {
        // Regression for issue #4: when the user fills both Text and Subject
        // in the popup and Subject contains spaces, the popup must quote the
        // value so it survives tokenization as a single phrase.
        let entries = vec![
            make_entry("alice@x.com", "Monthly Report Q1", "2024-01-15"),
            make_entry("bob@x.com", "Monthly Update", "2024-01-20"),
        ];
        // Simulates what the popup now emits for Text="alice" + Subject="Monthly Report"
        let q = parse_query("alice subject:\"Monthly Report\"");
        let results = search_metadata(&entries, &q);
        assert_eq!(results, vec![0]);
    }

    #[test]
    fn test_size_filter() {
        let mut entries = vec![
            make_entry("a@x.com", "Small", "2024-01-01"),
            make_entry("b@x.com", "Big", "2024-01-02"),
        ];
        entries[0].length = 500;
        entries[1].length = 5_000_000;

        let q = parse_query("size:>1mb");
        let results = search_metadata(&entries, &q);
        assert_eq!(results, vec![1]);
    }
}
