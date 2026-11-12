//! Full-text streaming search — reads message bodies from the MBOX file.
//!
//! This is the slow path: for each candidate message, it reads and decodes
//! the MIME body, then searches the plain text. Use metadata search first
//! to reduce the candidate set.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use tracing::debug;

use crate::model::mail::MailEntry;
use crate::store::reader::MboxStore;

use super::metadata::{all_matches_metadata, term_matches_entry};
use super::query::{SearchField, SearchOperator, SearchQuery, SearchTerm, TermGroup};

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
    // Groups the metadata pass could not settle: those holding a body:,
    // filename:, or free-text term (the last matches metadata *or* body).
    // Every one of them still has to hold, so they are AND-ed, while the terms
    // inside each are OR-ed — the same shape the metadata pass applies.
    let scan_groups: Vec<&TermGroup> = query.groups.iter().filter(|g| g.needs_body()).collect();

    if scan_groups.is_empty() {
        // Nothing to scan — all candidates pass
        return Ok(candidates.to_vec());
    }

    let total = candidates.len();
    if !progress(0, total) {
        debug!("Full-text search cancelled before it started");
        return Ok(Vec::new());
    }

    // Reading and decoding messages is the whole cost, and each message is
    // independent: spread them over the cores. Workers take the next block
    // of candidates from a shared counter, so a stretch of large messages
    // does not leave one of them behind.
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(MAX_WORKERS)
        .min(total.div_ceil(BLOCK))
        .max(1);
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let cancelled = AtomicBool::new(false);
    let (finished_tx, finished_rx) = std::sync::mpsc::channel::<()>();

    let mut matched: Vec<usize> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                let finished = finished_tx.clone();
                let (next, done, cancelled, scan_groups) = (&next, &done, &cancelled, &scan_groups);
                scope.spawn(move || {
                    let found = scan_blocks(
                        mbox_path,
                        entries,
                        candidates,
                        scan_groups,
                        next,
                        done,
                        cancelled,
                    );
                    let _ = finished.send(());
                    found
                })
            })
            .collect();
        drop(finished_tx);

        // The caller's callback is not required to be thread-safe, so it is
        // only ever called from here, a few times a second.
        let mut running = workers;
        while running > 0 {
            match finished_rx.recv_timeout(std::time::Duration::from_millis(50)) {
                Ok(()) => running -= 1,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if !cancelled.load(Ordering::Relaxed)
                && !progress(done.load(Ordering::Relaxed).min(total), total)
            {
                debug!("Full-text search cancelled");
                cancelled.store(true, Ordering::Relaxed);
            }
        }

        handles
            .into_iter()
            .map(|handle| handle.join())
            .collect::<std::result::Result<Vec<_>, _>>()
            .map(|parts| parts.into_iter().flatten().collect())
            .unwrap_or_else(|_| {
                // A worker panicked: report nothing rather than a partial
                // answer that looks complete.
                cancelled.store(true, Ordering::Relaxed);
                Vec::new()
            })
    });

    if cancelled.load(Ordering::Relaxed) {
        // A cancelled search has no result: whatever was found so far is an
        // arbitrary part of the answer.
        return Ok(Vec::new());
    }

    // Back to the order of `candidates`.
    matched.sort_unstable();
    let results = matched.into_iter().map(|pos| candidates[pos]).collect();

    // Final progress report
    let _ = progress(total, total);

    Ok(results)
}

/// Most worker threads a full-text search uses.
const MAX_WORKERS: usize = 8;
/// Candidates a worker takes at a time.
const BLOCK: usize = 32;

/// One worker of [`search_fulltext`]: take blocks of candidates until none
/// are left (or the search is cancelled) and return the positions, within
/// `candidates`, of those that match.
fn scan_blocks(
    mbox_path: &Path,
    entries: &[MailEntry],
    candidates: &[usize],
    scan_groups: &[&TermGroup],
    next: &AtomicUsize,
    done: &AtomicUsize,
    cancelled: &AtomicBool,
) -> Vec<usize> {
    let mut found = Vec::new();
    // Opened lazily: a query settled by metadata never touches the file.
    let mut store: Option<MboxStore> = None;
    loop {
        let start = next.fetch_add(BLOCK, Ordering::Relaxed);
        if start >= candidates.len() || cancelled.load(Ordering::Relaxed) {
            return found;
        }
        let end = (start + BLOCK).min(candidates.len());
        for (pos, &idx) in candidates.iter().enumerate().take(end).skip(start) {
            if cancelled.load(Ordering::Relaxed) {
                return found;
            }
            let entry = &entries[idx];
            let matches = if settled_by_metadata(entry, scan_groups) {
                true
            } else {
                if store.is_none() {
                    match MboxStore::open(mbox_path) {
                        Ok(opened) => store = Some(opened),
                        Err(e) => {
                            debug!(error = %e, "Cannot open the mailbox for full-text search");
                            cancelled.store(true, Ordering::Relaxed);
                            return found;
                        }
                    }
                }
                match store.as_mut() {
                    Some(store) => check_body_match(store, entry, scan_groups).unwrap_or_else(|e| {
                        debug!(offset = entry.offset, error = %e, "Skipping message in fulltext search");
                        false
                    }),
                    None => false,
                }
            };
            if matches {
                found.push(pos);
            }
            done.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Whether the message already satisfies every deferred group through its
/// index entry alone, so its body need not be read.
///
/// A group holds as soon as one of its terms does. `body:` and `filename:`
/// terms cannot be judged here, nor a negated free-text term (it needs the
/// body to *not* contain the word); a plain free-text term that matches the
/// subject or an address settles its group, and so does any metadata term
/// sharing a group with body ones (`body:x OR subject:y`).
fn settled_by_metadata(entry: &MailEntry, scan_groups: &[&TermGroup]) -> bool {
    scan_groups.iter().all(|group| {
        group.terms.iter().any(|term| match term.field {
            SearchField::Body | SearchField::Filename => false,
            SearchField::All => !term.negated && all_matches_metadata(entry, &term.operator),
            _ => term_matches_entry(entry, term),
        })
    })
}

/// Check whether a single message satisfies the deferred groups by reading its
/// body.
///
/// Handles `body:`, `filename:`, and free-text `All` terms. `All` terms match
/// if the needle is found in the entry's metadata (subject/from/to) **or** the
/// decoded body text — the "search everywhere" semantics users expect. Every
/// group must hold (AND); within a group, any term suffices (OR).
fn check_body_match(
    store: &mut MboxStore,
    entry: &MailEntry,
    scan_groups: &[&TermGroup],
) -> crate::error::Result<bool> {
    let body = store.get_message_uncached(entry)?;
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
            // Free-text term: match metadata or body.
            SearchField::All => {
                all_matches_metadata(entry, &term.operator)
                    || match &term.operator {
                        SearchOperator::Contains(needle) | SearchOperator::Exact(needle) => {
                            text_lower.contains(needle)
                        }
                    }
            }
            // A deferred group can hold metadata terms beside the body ones
            // (`body:x OR subject:y`); those are judged exactly as the
            // metadata pass would. `term_matches_entry` applies negation
            // itself, so return its verdict directly.
            _ => return term_matches_entry(entry, term),
        };

        if term.negated {
            !raw_match
        } else {
            raw_match
        }
    };

    let result = scan_groups
        .iter()
        .all(|group| group.terms.iter().any(&check_term));

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
    fn test_fulltext_matches_a_serial_scan_in_candidate_order() {
        // Enough messages for several workers and blocks; every third one
        // carries the needle, in the body only.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("many.mbox");
        let mut data = String::new();
        for i in 0..500 {
            let word = if i % 3 == 0 { "needleword" } else { "nothing" };
            data.push_str(&format!(
                "From a@b Thu Jan 01 00:00:00 2024\nSubject: message {i}\nMessage-ID: <{i}@x>\n\nbody {word} here\n\n"
            ));
        }
        std::fs::write(&path, data).unwrap();
        let entries = builder::build_index(&path, true, None).unwrap();
        assert_eq!(entries.len(), 500);
        // Candidates in a deliberately odd order.
        let candidates: Vec<usize> = (0..500).rev().collect();

        let query = parse_query("body:needleword");
        let results = search_fulltext(&path, &entries, &candidates, &query, &|_, _| true).unwrap();
        let expected: Vec<usize> = candidates.iter().copied().filter(|i| i % 3 == 0).collect();
        assert_eq!(results, expected);

        // Progress ends at (total, total) and never goes past it.
        let last = std::sync::Mutex::new((0usize, 0usize));
        search_fulltext(&path, &entries, &candidates, &query, &|done, total| {
            assert!(done <= total);
            *last.lock().unwrap() = (done, total);
            true
        })
        .unwrap();
        assert_eq!(*last.lock().unwrap(), (500, 500));
    }

    #[test]
    fn test_free_text_settled_by_metadata_needs_no_body() {
        let mbox_path = fixture("simple.mbox");
        let entries = builder::build_index(&mbox_path, true, None).unwrap();
        let word = entries[0]
            .subject
            .split_whitespace()
            .next()
            .unwrap()
            .to_lowercase();
        let query = parse_query(&word);
        let groups: Vec<&TermGroup> = query.groups.iter().filter(|g| g.needs_body()).collect();
        assert!(settled_by_metadata(&entries[0], &groups));
        // `body:` and negated free text always need the body.
        for q in [format!("body:{word}"), format!("-{word}")] {
            let query = parse_query(&q);
            let groups: Vec<&TermGroup> = query.groups.iter().filter(|g| g.needs_body()).collect();
            assert!(!settled_by_metadata(&entries[0], &groups), "{q}");
        }
        // The result is the same as reading every body: point the search at
        // a mailbox that does not exist and it still finds the message.
        let candidates = vec![0usize];
        let found = search_fulltext(
            Path::new("/nonexistent/mailbox.mbox"),
            &entries,
            &candidates,
            &query,
            &|_, _| true,
        )
        .unwrap();
        assert_eq!(found, vec![0]);
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
