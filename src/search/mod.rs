//! Search engine: query parsing, metadata search, and full-text search.

pub mod fulltext;
pub mod metadata;
pub mod query;

use std::path::Path;

use crate::model::mail::MailEntry;

use self::query::{parse_query, SearchQuery};

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

    // Phase 1: metadata search (fast)
    let mut results = metadata::search_metadata(entries, &query);

    // Phase 2: full-text search if needed
    if query.needs_fulltext {
        let progress_fn = progress.unwrap_or(&|_, _| true);
        results = fulltext::search_fulltext(mbox_path, entries, &results, &query, progress_fn)?;
    }

    Ok((query, results))
}
