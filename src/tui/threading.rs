//! JWZ-based email threading algorithm.
//!
//! Groups messages into conversation threads using `Message-ID`,
//! `In-Reply-To`, and `References` headers.
//!
//! Reference: <https://www.jwz.org/doc/threading.html>

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::model::mail::MailEntry;

/// A complete thread of related messages.
#[derive(Debug)]
pub struct Thread {
    /// `Message-ID` of the root message (or a synthetic ID for orphan roots).
    pub root_message_id: String,
    /// Normalized subject of the thread.
    pub subject: String,
    /// Total number of messages in the thread.
    pub total_count: usize,
    /// Date range: (oldest, newest).
    pub date_range: (DateTime<Utc>, DateTime<Utc>),
    /// Flattened list of `(entry_index, depth)` pairs, pre-ordered.
    pub nodes: Vec<(usize, usize)>,
}

/// Internal container used during the threading algorithm.
#[derive(Debug)]
struct Container {
    /// Index into the `entries` slice, if this container has an actual message.
    entry_index: Option<usize>,
    /// `Message-ID` for this container.
    message_id: String,
    /// Parent container `Message-ID`, if known.
    parent: Option<String>,
    /// Children container `Message-ID`s.
    children: Vec<String>,
}

/// Build threads from a slice of mail entries.
///
/// Returns threads sorted by the newest message date (descending).
pub fn build_threads(entries: &[MailEntry]) -> Vec<Thread> {
    if entries.is_empty() {
        return Vec::new();
    }

    // Step 1: Build a container for every known Message-ID.
    let mut containers: HashMap<String, Container> = HashMap::new();

    for (idx, entry) in entries.iter().enumerate() {
        let mid = normalize_id(&entry.message_id);
        if mid.is_empty() {
            // Synthesize an ID for messages without one
            let synth = format!("__synth_{}__", idx);
            containers.insert(
                synth.clone(),
                Container {
                    entry_index: Some(idx),
                    message_id: synth,
                    parent: None,
                    children: Vec::new(),
                },
            );
            continue;
        }

        // Get or create this message's container
        let c = containers.entry(mid.clone()).or_insert_with(|| Container {
            entry_index: None,
            message_id: mid.clone(),
            parent: None,
            children: Vec::new(),
        });
        c.entry_index = Some(idx);

        // Step 2: Link references chain.
        // References: A B C D means A→B→C→D→this_message
        let mut refs: Vec<String> = entry
            .references
            .iter()
            .map(|r| normalize_id(r))
            .filter(|r| !r.is_empty())
            .collect();

        if let Some(reply_to) = &entry.in_reply_to {
            let nid = normalize_id(reply_to);
            if !nid.is_empty() && !refs.contains(&nid) {
                refs.push(nid);
            }
        }

        // Ensure all referenced IDs have containers
        for rid in &refs {
            containers.entry(rid.clone()).or_insert_with(|| Container {
                entry_index: None,
                message_id: rid.clone(),
                parent: None,
                children: Vec::new(),
            });
        }

        // Link the chain: refs[0]→refs[1]→...→refs[n]→mid
        let mut chain: Vec<String> = refs;
        chain.push(mid.clone());

        for window in chain.windows(2) {
            let parent_id = &window[0];
            let child_id = &window[1];

            if parent_id == child_id {
                continue;
            }

            // Check for cycles before linking
            if would_create_cycle(&containers, parent_id, child_id) {
                continue;
            }

            // Remove child from its old parent (if any)
            if let Some(old_parent_id) = containers.get(child_id).and_then(|c| c.parent.clone()) {
                if old_parent_id != *parent_id {
                    if let Some(old_parent) = containers.get_mut(&old_parent_id) {
                        old_parent.children.retain(|c| c != child_id);
                    }
                }
            }

            // Set new parent
            if let Some(child) = containers.get_mut(child_id) {
                child.parent = Some(parent_id.clone());
            }
            if let Some(parent) = containers.get_mut(parent_id) {
                if !parent.children.contains(child_id) {
                    parent.children.push(child_id.clone());
                }
            }
        }
    }

    // Step 3: Find root containers (no parent).
    let root_ids: Vec<String> = containers
        .values()
        .filter(|c| c.parent.is_none())
        .map(|c| c.message_id.clone())
        .collect();

    // Step 4: Prune empty containers with a single child — promote the child.
    // (We skip full pruning for simplicity and just collect threads from roots.)

    // Step 5: Group by subject (merge roots with same normalized subject).
    let mut subject_map: HashMap<String, Vec<String>> = HashMap::new();
    for rid in &root_ids {
        let subj = root_subject(rid, &containers, entries);
        subject_map.entry(subj).or_default().push(rid.clone());
    }

    // Build Thread objects
    let mut threads: Vec<Thread> = Vec::new();

    for (subject, group_root_ids) in &subject_map {
        let mut nodes: Vec<(usize, usize)> = Vec::new();
        let mut oldest = DateTime::<Utc>::MAX_UTC;
        let mut newest = DateTime::<Utc>::MIN_UTC;

        for rid in group_root_ids {
            flatten(
                rid,
                0,
                &containers,
                entries,
                &mut nodes,
                &mut oldest,
                &mut newest,
            );
        }

        if nodes.is_empty() {
            continue;
        }

        // Sort nodes within thread by date
        nodes.sort_by(|a, b| entries[a.0].date.cmp(&entries[b.0].date));

        threads.push(Thread {
            root_message_id: group_root_ids[0].clone(),
            subject: subject.clone(),
            total_count: nodes.len(),
            date_range: (oldest, newest),
            nodes,
        });
    }

    // Sort threads by newest message date (descending)
    threads.sort_by(|a, b| b.date_range.1.cmp(&a.date_range.1));

    threads
}

/// Flatten visible indices from threads into a single list with depth info.
///
/// Returns `(entry_index, depth)` pairs suitable for display in the mail list.
pub fn flatten_threads_to_indices(threads: &[Thread]) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    for thread in threads {
        result.extend_from_slice(&thread.nodes);
    }
    result
}

/// Check if making `parent_id` the parent of `child_id` would create a cycle.
fn would_create_cycle(
    containers: &HashMap<String, Container>,
    parent_id: &str,
    child_id: &str,
) -> bool {
    // Walk up from parent_id; if we reach child_id, it's a cycle
    let mut current = Some(parent_id.to_string());
    let mut depth = 0;
    while let Some(ref id) = current {
        if id == child_id {
            return true;
        }
        depth += 1;
        if depth > 100 {
            // Safety: break on deep chains
            return true;
        }
        current = containers.get(id.as_str()).and_then(|c| c.parent.clone());
    }
    false
}

/// Recursively flatten a container and its children into `(entry_index, depth)`.
fn flatten(
    id: &str,
    depth: usize,
    containers: &HashMap<String, Container>,
    entries: &[MailEntry],
    out: &mut Vec<(usize, usize)>,
    oldest: &mut DateTime<Utc>,
    newest: &mut DateTime<Utc>,
) {
    let Some(container) = containers.get(id) else {
        return;
    };

    if let Some(idx) = container.entry_index {
        out.push((idx, depth));
        let d = entries[idx].date;
        if d < *oldest {
            *oldest = d;
        }
        if d > *newest {
            *newest = d;
        }
    }

    // Sort children by date before recursing
    let mut children = container.children.clone();
    children.sort_by(|a, b| {
        let da = containers
            .get(a.as_str())
            .and_then(|c| c.entry_index)
            .map(|i| entries[i].date);
        let db = containers
            .get(b.as_str())
            .and_then(|c| c.entry_index)
            .map(|i| entries[i].date);
        da.cmp(&db)
    });

    let child_depth = if container.entry_index.is_some() {
        depth + 1
    } else {
        depth
    };

    for child_id in &children {
        flatten(
            child_id,
            child_depth,
            containers,
            entries,
            out,
            oldest,
            newest,
        );
    }
}

/// Get the normalized subject for a root container.
fn root_subject(
    root_id: &str,
    containers: &HashMap<String, Container>,
    entries: &[MailEntry],
) -> String {
    if let Some(c) = containers.get(root_id) {
        if let Some(idx) = c.entry_index {
            return normalize_subject(&entries[idx].subject);
        }
        // If root has no message, try first child
        for child_id in &c.children {
            if let Some(child) = containers.get(child_id.as_str()) {
                if let Some(idx) = child.entry_index {
                    return normalize_subject(&entries[idx].subject);
                }
            }
        }
    }
    String::new()
}

/// Normalize a Message-ID by stripping angle brackets and whitespace.
fn normalize_id(id: &str) -> String {
    id.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim()
        .to_string()
}

/// Normalize a subject for grouping: strip Re:/Fwd: prefixes, lowercase.
fn normalize_subject(subject: &str) -> String {
    let mut s = subject.trim().to_string();
    loop {
        let lower = s.trim().to_lowercase();
        if lower.starts_with("re:") {
            s = s.trim()[3..].trim_start().to_string();
        } else if lower.starts_with("fwd:") {
            s = s.trim()[4..].trim_start().to_string();
        } else if lower.starts_with("fw:") {
            s = s.trim()[3..].trim_start().to_string();
        } else {
            break;
        }
    }
    s.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::address::EmailAddress;
    use chrono::TimeZone;

    fn make_entry(
        idx: u64,
        message_id: &str,
        in_reply_to: Option<&str>,
        references: Vec<&str>,
        subject: &str,
        date: DateTime<Utc>,
    ) -> MailEntry {
        MailEntry {
            offset: idx * 1000,
            length: 500,
            date,
            from: EmailAddress {
                display_name: String::new(),
                address: format!("user{}@example.com", idx),
            },
            to: Vec::new(),
            cc: Vec::new(),
            subject: subject.to_string(),
            message_id: message_id.to_string(),
            in_reply_to: in_reply_to.map(String::from),
            references: references.into_iter().map(String::from).collect(),
            has_attachments: false,
            content_type: "text/plain".to_string(),
            text_size: 100,
            labels: Vec::new(),
            sequence: idx,
        }
    }

    #[test]
    fn test_normalize_subject() {
        assert_eq!(normalize_subject("Hello"), "hello");
        assert_eq!(normalize_subject("Re: Hello"), "hello");
        assert_eq!(normalize_subject("Re: Re: Hello"), "hello");
        assert_eq!(normalize_subject("Fwd: Hello"), "hello");
        assert_eq!(normalize_subject("FW: Re: Hello"), "hello");
    }

    #[test]
    fn test_normalize_id() {
        assert_eq!(normalize_id("<msg001@example.com>"), "msg001@example.com");
        assert_eq!(normalize_id("msg001@example.com"), "msg001@example.com");
        assert_eq!(normalize_id("  <msg@ex.com>  "), "msg@ex.com");
    }

    #[test]
    fn test_empty_entries() {
        let threads = build_threads(&[]);
        assert!(threads.is_empty());
    }

    #[test]
    fn test_single_message_thread() {
        let entries = vec![make_entry(
            0,
            "<msg001@ex.com>",
            None,
            vec![],
            "Hello",
            Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 0).unwrap(),
        )];
        let threads = build_threads(&entries);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].total_count, 1);
        assert_eq!(threads[0].nodes[0], (0, 0));
    }

    #[test]
    fn test_simple_reply_thread() {
        let entries = vec![
            make_entry(
                0,
                "<msg001@ex.com>",
                None,
                vec![],
                "Hello",
                Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 0).unwrap(),
            ),
            make_entry(
                1,
                "<msg002@ex.com>",
                Some("<msg001@ex.com>"),
                vec!["<msg001@ex.com>"],
                "Re: Hello",
                Utc.with_ymd_and_hms(2024, 1, 1, 11, 0, 0).unwrap(),
            ),
        ];
        let threads = build_threads(&entries);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].total_count, 2);
        // Root at depth 0, reply at depth 1
        assert_eq!(threads[0].nodes[0].0, 0);
        assert_eq!(threads[0].nodes[0].1, 0);
        assert_eq!(threads[0].nodes[1].0, 1);
        assert_eq!(threads[0].nodes[1].1, 1);
    }

    #[test]
    fn test_unrelated_messages_are_separate_threads() {
        let entries = vec![
            make_entry(
                0,
                "<msg001@ex.com>",
                None,
                vec![],
                "Topic A",
                Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 0).unwrap(),
            ),
            make_entry(
                1,
                "<msg002@ex.com>",
                None,
                vec![],
                "Topic B",
                Utc.with_ymd_and_hms(2024, 1, 1, 11, 0, 0).unwrap(),
            ),
        ];
        let threads = build_threads(&entries);
        assert_eq!(threads.len(), 2);
    }

    #[test]
    fn test_deep_thread_chain() {
        let entries = vec![
            make_entry(
                0,
                "<a@ex.com>",
                None,
                vec![],
                "Topic",
                Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 0).unwrap(),
            ),
            make_entry(
                1,
                "<b@ex.com>",
                Some("<a@ex.com>"),
                vec!["<a@ex.com>"],
                "Re: Topic",
                Utc.with_ymd_and_hms(2024, 1, 1, 11, 0, 0).unwrap(),
            ),
            make_entry(
                2,
                "<c@ex.com>",
                Some("<b@ex.com>"),
                vec!["<a@ex.com>", "<b@ex.com>"],
                "Re: Re: Topic",
                Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap(),
            ),
        ];
        let threads = build_threads(&entries);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].total_count, 3);
        // depths: 0, 1, 2
        assert_eq!(threads[0].nodes[0].1, 0);
        assert_eq!(threads[0].nodes[1].1, 1);
        assert_eq!(threads[0].nodes[2].1, 2);
    }

    #[test]
    fn test_subject_grouping() {
        // Two messages with same subject but no references should still be grouped
        let entries = vec![
            make_entry(
                0,
                "<a@ex.com>",
                None,
                vec![],
                "Meeting",
                Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 0).unwrap(),
            ),
            make_entry(
                1,
                "<b@ex.com>",
                None,
                vec![],
                "Re: Meeting",
                Utc.with_ymd_and_hms(2024, 1, 1, 11, 0, 0).unwrap(),
            ),
        ];
        let threads = build_threads(&entries);
        // Should be merged into one thread by subject
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].total_count, 2);
    }

    #[test]
    fn test_flatten_threads_to_indices() {
        let entries = vec![
            make_entry(
                0,
                "<a@ex.com>",
                None,
                vec![],
                "Topic",
                Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 0).unwrap(),
            ),
            make_entry(
                1,
                "<b@ex.com>",
                Some("<a@ex.com>"),
                vec!["<a@ex.com>"],
                "Re: Topic",
                Utc.with_ymd_and_hms(2024, 1, 1, 11, 0, 0).unwrap(),
            ),
        ];
        let threads = build_threads(&entries);
        let flat = flatten_threads_to_indices(&threads);
        assert_eq!(flat.len(), 2);
    }
}
