//! Index querying utilities.

use crate::model::mail::MailEntry;

/// Sort entries by date (newest first by default).
pub fn sort_by_date(entries: &[MailEntry], ascending: bool) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..entries.len()).collect();
    indices.sort_by(|&a, &b| {
        let cmp = entries[a].date.cmp(&entries[b].date);
        if ascending {
            cmp
        } else {
            cmp.reverse()
        }
    });
    indices
}

/// Return the date range (oldest, newest) across the given entries.
pub fn date_range(
    entries: &[MailEntry],
) -> Option<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)> {
    if entries.is_empty() {
        return None;
    }
    let mut min = entries[0].date;
    let mut max = entries[0].date;
    for e in entries.iter().skip(1) {
        if e.date < min {
            min = e.date;
        }
        if e.date > max {
            max = e.date;
        }
    }
    Some((min, max))
}

/// Count how many entries have attachments.
pub fn count_with_attachments(entries: &[MailEntry]) -> usize {
    entries.iter().filter(|e| e.has_attachments).count()
}

/// Return the top N senders by message count.
pub fn top_senders(entries: &[MailEntry], n: usize) -> Vec<(String, usize)> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for entry in entries {
        let key = if entry.from.display_name.is_empty() {
            entry.from.address.clone()
        } else {
            entry.from.display()
        };
        *counts.entry(key).or_default() += 1;
    }
    let mut sorted: Vec<(String, usize)> = counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    sorted.truncate(n);
    sorted
}
