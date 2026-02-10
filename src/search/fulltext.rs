//! Full-text streaming search — reads message bodies from the MBOX file.
//!
//! This is the slow path: for each candidate message, it reads and decodes
//! the MIME body, then searches the plain text. Use metadata search first
//! to reduce the candidate set.

use std::path::Path;

use tracing::debug;

use crate::model::mail::MailEntry;
use crate::store::reader::MboxStore;

use super::query::{SearchField, SearchOperator, SearchQuery, SearchTerm};

/// Search inside message bodies by reading from the MBOX file.
///
/// `candidates` is a pre-filtered list of entry indices (from metadata search).
/// The progress callback receives `(processed, total)` and should return
/// `true` to continue or `false` to cancel.
///
/// Returns the indices that match the full-text criteria.
pub fn search_fulltext(
    mbox_path: &Path,
    entries: &[MailEntry],
    candidates: &[usize],
    query: &SearchQuery,
    progress: &dyn Fn(usize, usize) -> bool,
) -> crate::error::Result<Vec<usize>> {
    let body_terms: Vec<&SearchTerm> = query
        .terms
        .iter()
        .filter(|t| t.field == SearchField::Body || t.field == SearchField::Filename)
        .collect();

    if body_terms.is_empty() {
        // No body terms — all candidates pass
        return Ok(candidates.to_vec());
    }

    let mut store = MboxStore::open(mbox_path)?;
    let mut results = Vec::new();
    let total = candidates.len();

    for (i, &idx) in candidates.iter().enumerate() {
        // Report progress and check for cancellation
        if !progress(i, total) {
            debug!("Full-text search cancelled at {i}/{total}");
            break;
        }

        let entry = &entries[idx];
        let matches = match check_body_match(&mut store, entry, &body_terms, query.is_or) {
            Ok(m) => m,
            Err(e) => {
                debug!(offset = entry.offset, error = %e, "Skipping message in fulltext search");
                false
            }
        };

        if matches {
            results.push(idx);
        }
    }

    // Final progress report
    let _ = progress(total, total);

    Ok(results)
}

/// Check whether a single message's body matches the body search terms.
fn check_body_match(
    store: &mut MboxStore,
    entry: &MailEntry,
    body_terms: &[&SearchTerm],
    is_or: bool,
) -> crate::error::Result<bool> {
    let body = store.get_message(entry)?;
    let text = body.text.as_deref().unwrap_or("");

    let text_lower = text.to_lowercase();

    let check_term = |term: &SearchTerm| -> bool {
        let raw_match = match term.field {
            SearchField::Body => match &term.operator {
                SearchOperator::Contains(needle) => text_lower.contains(needle),
                SearchOperator::Exact(phrase) => text_lower.contains(phrase),
            },
            SearchField::Filename => {
                // Search in attachment filenames
                body.attachments.iter().any(|att| {
                    let fname = att.filename.to_lowercase();
                    match &term.operator {
                        SearchOperator::Contains(needle) => fname.contains(needle),
                        SearchOperator::Exact(phrase) => fname == *phrase,
                    }
                })
            }
            _ => true,
        };

        if term.negated {
            !raw_match
        } else {
            raw_match
        }
    };

    let result = if is_or {
        body_terms.iter().any(|t| check_term(t))
    } else {
        body_terms.iter().all(|t| check_term(t))
    };

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::builder;
    use crate::search::query::parse_query;
    use std::path::Path;

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name)
    }

    #[test]
    fn test_fulltext_body_search() {
        let mbox_path = fixture("simple.mbox");
        let entries = builder::build_index(&mbox_path, true, None).unwrap();
        let candidates: Vec<usize> = (0..entries.len()).collect();

        let query = parse_query("body:especiales");
        let results =
            search_fulltext(&mbox_path, &entries, &candidates, &query, &|_, _| true).unwrap();

        // The third message has "caracteres especiales" in body
        assert!(
            !results.is_empty(),
            "Should find at least one message with 'especiales' in body"
        );
    }

    #[test]
    fn test_fulltext_cancellation() {
        let mbox_path = fixture("simple.mbox");
        let entries = builder::build_index(&mbox_path, true, None).unwrap();
        let candidates: Vec<usize> = (0..entries.len()).collect();

        let query = parse_query("body:test");
        // Cancel immediately
        let results =
            search_fulltext(&mbox_path, &entries, &candidates, &query, &|_, _| false).unwrap();

        assert!(results.is_empty(), "Cancelled search should return empty");
    }

    #[test]
    fn test_fulltext_no_body_terms_passes_all() {
        let mbox_path = fixture("simple.mbox");
        let entries = builder::build_index(&mbox_path, true, None).unwrap();
        let candidates: Vec<usize> = (0..entries.len()).collect();

        // Query with no body: terms
        let query = parse_query("from:user1");
        let results =
            search_fulltext(&mbox_path, &entries, &candidates, &query, &|_, _| true).unwrap();

        assert_eq!(results.len(), candidates.len(), "No body terms → all pass");
    }
}
