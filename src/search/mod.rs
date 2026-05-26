//! Search engine: query parsing, metadata search, and full-text search.

pub mod fulltext;
pub mod metadata;
pub mod query;

use std::path::Path;

use crate::model::mail::MailEntry;

use self::query::{parse_query, SearchField, SearchQuery};

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
    let has_all_text = query.terms.iter().any(|t| t.field == SearchField::All);
    let scan_bodies = query.needs_fulltext || (has_all_text && !query.is_or);

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
}
