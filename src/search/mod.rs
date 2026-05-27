//! Search engine: query parsing, metadata search, and full-text search.

pub mod fulltext;
pub mod metadata;
pub mod query;

use std::path::Path;

use crate::model::mail::MailEntry;

use self::query::{parse_query, SearchField, SearchQuery};

/// Whether running this query requires reading message bodies from disk
/// (the slow, cancelable path).
///
/// True when an explicit `body:`/`filename:` term is present, or when a
/// free-text (`All`) term is used in an AND query — those search the body
/// as well as metadata. OR queries keep `All` terms metadata-only, so they
/// never need a body scan on their own.
pub fn needs_body_scan(query: &SearchQuery) -> bool {
    let has_all_text = query.terms.iter().any(|t| t.field == SearchField::All);
    query.needs_fulltext || (has_all_text && !query.is_or)
}

/// High-level search: parse the query, search metadata, optionally run
/// full-text search, and return matching entry indices.
///
/// The `progress` callback is only invoked for full-text searches.
/// It receives `(processed, total)` and returns `false` to cancel.
pub fn execute(
    mbox_path: &Path,
    entries: &[MailEntry],
    query_str: &str,
    progress: Option<&dyn Fn(usize, usize) -> bool>,
) -> crate::error::Result<(SearchQuery, Vec<usize>)> {
    let query = parse_query(query_str);

    if query.terms.is_empty()
        && query.date_filter.is_none()
        && query.size_filter.is_none()
        && query.has_attachment.is_none()
    {
        // Empty query — return all
        let all: Vec<usize> = (0..entries.len()).collect();
        return Ok((query, all));
    }

    // Free-text ("Text") terms search everywhere — subject/from/to AND the
    // message body — but only on AND queries (the popup always emits AND).
    // On OR queries `All` terms keep metadata-only semantics to avoid dropping
    // metadata-only matches from the candidate set.
    let scan_bodies = needs_body_scan(&query);

    // Phase 1: metadata search (fast). When we will scan bodies for `All`
    // terms, defer them so a Text term that only appears in the body is not
    // filtered out before the body is ever read.
    let mut results = if scan_bodies && !query.is_or {
        metadata::search_metadata_candidates(entries, &query)
    } else {
        metadata::search_metadata(entries, &query)
    };

    // Phase 2: full-text search if any term needs the body (body:/filename:
    // or a deferred Text term).
    if scan_bodies {
        let progress_fn = progress.unwrap_or(&|_, _| true);
        results = fulltext::search_fulltext(mbox_path, entries, &results, &query, progress_fn)?;
    }

    Ok((query, results))
}

#[cfg(test)]
mod tests {
    use crate::index::builder;
    use std::path::{Path, PathBuf};

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name)
    }

    /// Collect the subjects of the entries returned by a search.
    fn search_subjects(query: &str) -> Vec<String> {
        let mbox_path = fixture("simple.mbox");
        let entries = builder::build_index(&mbox_path, true, None).unwrap();
        let (_q, results) = super::execute(&mbox_path, &entries, query, None).unwrap();
        results
            .iter()
            .map(|&i| entries[i].subject.clone())
            .collect()
    }

    #[test]
    fn test_free_text_matches_body() {
        // Regression for issues #4/#6: a bare "Text" term must find a word that
        // lives only in the message body (msg004 has "perspective" in its body,
        // not its subject/from/to).
        let subjects = search_subjects("perspective");
        assert_eq!(subjects, vec!["Message with From in body".to_string()]);
    }

    #[test]
    fn test_free_text_plus_subject_matches_body() {
        // Regression for issue #4: combining Text (body word) + Subject must
        // match. This is exactly what the Search Filters popup emits.
        let subjects = search_subjects("perspective subject:\"From in body\"");
        assert_eq!(subjects, vec!["Message with From in body".to_string()]);
    }

    #[test]
    fn test_free_text_still_matches_metadata() {
        // Text terms that live in metadata must keep working (no regression).
        let mut subjects = search_subjects("Hello subject:World");
        subjects.sort();
        assert_eq!(
            subjects,
            vec!["Hello World".to_string(), "Re: Hello World".to_string()]
        );
    }

    #[test]
    fn test_free_text_no_match_anywhere() {
        let subjects = search_subjects("zzzznotfoundanywhere");
        assert!(subjects.is_empty());
    }

    #[test]
    fn test_negated_free_text_excludes_body_match() {
        // `-perspective` must exclude the one message whose body has it.
        let subjects = search_subjects("-perspective");
        assert_eq!(subjects.len(), 4);
        assert!(!subjects.contains(&"Message with From in body".to_string()));
    }

    #[test]
    fn test_multiword_free_text_ands_across_metadata_and_body() {
        // Issue #6 follow-up: a multi-word Text search must AND each word and
        // search everywhere. msg004 has "perspective" in its body and
        // "Message" in its subject, so both words match the same message.
        let subjects = search_subjects("perspective message");
        assert_eq!(subjects, vec!["Message with From in body".to_string()]);
    }

    #[test]
    fn test_multiword_free_text_requires_every_word() {
        // If any word matches nowhere, the AND query returns nothing — even
        // though "perspective" alone would match.
        let subjects = search_subjects("perspective zzzznotfoundanywhere");
        assert!(subjects.is_empty());
    }

    #[test]
    fn test_needs_body_scan_classification() {
        use super::query::parse_query;
        // Free-text (single or multi-word) needs the body scan.
        assert!(super::needs_body_scan(&parse_query("hello")));
        assert!(super::needs_body_scan(&parse_query("multi word search")));
        // Explicit body:/filename: too.
        assert!(super::needs_body_scan(&parse_query("body:x")));
        assert!(super::needs_body_scan(&parse_query("filename:report.pdf")));
        // Metadata-only queries never scan bodies.
        assert!(!super::needs_body_scan(&parse_query("from:a@b.com")));
        assert!(!super::needs_body_scan(&parse_query("subject:hello")));
        assert!(!super::needs_body_scan(&parse_query("has:attachment")));
        // OR free-text stays metadata-only.
        assert!(!super::needs_body_scan(&parse_query("from:a OR from:b")));
    }
}
