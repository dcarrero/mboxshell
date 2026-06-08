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

/// Count duplicate messages based on `Message-ID`.
pub fn count_duplicates(entries: &[MailEntry]) -> (usize, usize) {
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut duplicates = 0usize;
    for e in entries {
        let id = e.message_id.as_str();
        if !id.is_empty() && !seen.insert(id) {
            duplicates += 1;
        }
    }
    let unique = seen.len();
    (duplicates, unique)
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
    sorted.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    sorted.truncate(n);
    sorted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::address::EmailAddress;
    use chrono::Utc;

    fn make_entry(idx: u64, message_id: &str) -> MailEntry {
        MailEntry {
            offset: idx * 1000,
            length: 500,
            date: Utc::now(),
            from: EmailAddress {
                display_name: String::new(),
                address: format!("user{}@example.com", idx),
            },
            to: Vec::new(),
            cc: Vec::new(),
            subject: String::new(),
            message_id: message_id.to_string(),
            in_reply_to: None,
            references: Vec::new(),
            has_attachments: false,
            content_type: "text/plain".to_string(),
            text_size: 100,
            labels: Vec::new(),
            sequence: idx,
        }
    }

    #[test]
    fn test_count_duplicates() {
        let entries = vec![
            make_entry(0, "<a@example.com>"),
            make_entry(1, "<b@example.com>"),
            make_entry(2, "<a@example.com>"), // duplicate of entry 0
            make_entry(3, ""),                // no message_id, not a duplicate
            make_entry(4, "<c@example.com>"),
            make_entry(5, "<b@example.com>"), // duplicate of entry 1
        ];
        let (duplicates, unique_ids) = count_duplicates(&entries);
        assert_eq!(duplicates, 2);
        assert_eq!(unique_ids, 3);
    }
}
