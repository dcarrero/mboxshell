//! Integration tests for Google Groups mailboxes shipped in Takeout exports.
//!
//! A Takeout archive contains two different kinds of mbox: the familiar Gmail
//! export, and one per group the account owns, at
//! `Groups/googlegroups.com/<group>@googlegroups.com/topics.mbox` (localised —
//! `Grupos/.../temas.mbox` in a Spanish export). See issue #23.

use std::path::{Path, PathBuf};

use mboxshell::mailbox_naming::{display_name, unique_display_names};
use mboxshell::parser::header::{parse_date, parse_headers_to_entry};
use mboxshell::parser::mbox::MboxParser;
use mboxshell::tui::threading::build_threads;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn entries() -> Vec<mboxshell::model::mail::MailEntry> {
    let parser = MboxParser::new(fixture("google_groups.mbox")).unwrap();
    let mut entries = Vec::new();
    let mut sequence = 0u64;
    parser
        .parse_headers_only(
            &mut |offset, length, header_bytes| {
                entries
                    .push(parse_headers_to_entry(header_bytes, offset, length, sequence).unwrap());
                sequence += 1;
                true
            },
            None,
        )
        .unwrap();
    entries
}

/// Google Groups writes the `From_` envelope date with the timezone offset
/// *before* the year, which asctime does not allow. It used to fall through to
/// a last-resort parse that invented a plausible-looking `2000-09-16`.
#[test]
fn test_groups_envelope_date_format() {
    let dt = parse_date("Apr 16 09:53:04 +0000 2015").expect("Groups envelope date must parse");
    assert_eq!(dt.to_rfc3339(), "2015-04-16T09:53:04+00:00");
}

/// Plain asctime must keep working: the new format sits after it and must not
/// shadow it.
#[test]
fn test_plain_asctime_still_parses() {
    let dt = parse_date("Apr 16 09:53:04 2015").expect("asctime must still parse");
    assert_eq!(dt.to_rfc3339(), "2015-04-16T09:53:04+00:00");
}

/// The first fixture message carries no `Date:` header at all, so its date can
/// only come from the `From_` line.
#[test]
fn test_message_without_date_header_uses_envelope_date() {
    let entries = entries();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].date.to_rfc3339(), "2015-04-16T09:53:04+00:00");
}

/// Groups messages have no `X-Gmail-Labels`; the group is surfaced as a
/// virtual label so the sidebar is populated for them too.
#[test]
fn test_group_is_exposed_as_a_label() {
    let entries = entries();
    assert_eq!(entries[0].labels, vec!["my-group".to_string()]);
    // Third message has no `X-Google-Groups`, only `X-BeenThere`.
    assert_eq!(entries[2].labels, vec!["my-group".to_string()]);
}

/// `X-BeenThere` is a generic mailing-list header; only googlegroups.com
/// addresses become labels.
#[test]
fn test_non_groups_mailing_list_is_not_labelled() {
    let entries = entries();
    assert!(entries.iter().all(|e| e.labels.len() <= 1));
}

#[test]
fn test_thread_id_is_captured() {
    let entries = entries();
    assert_eq!(entries[0].thread_id.as_deref(), Some("28616527183872"));
    assert_eq!(entries[1].thread_id.as_deref(), Some("28616527183872"));
    assert_eq!(entries[2].thread_id.as_deref(), Some("99999999999999"));
}

/// Messages 1 and 3 share a normalized subject but belong to different
/// conversations. Grouping by subject alone merged them; the explicit thread id
/// keeps them apart, while the real reply stays with its parent.
#[test]
fn test_thread_id_beats_subject_grouping() {
    let entries = entries();
    let threads = build_threads(&entries);
    assert_eq!(threads.len(), 2, "same subject, two conversations");

    let big = threads
        .iter()
        .find(|t| t.total_count == 2)
        .expect("root + reply belong together");
    // `Thread.subject` is the normalized (lowercased, `Re:`-stripped) form.
    assert_eq!(big.subject, "groups message with no date header");
    assert!(threads.iter().any(|t| t.total_count == 1));
}

/// The file name is always `topics.mbox` / `temas.mbox`; the group is the
/// directory around it.
#[test]
fn test_group_mailbox_is_not_named_topics() {
    let path =
        Path::new("/tmp/Takeout/Groups/googlegroups.com/my-group@googlegroups.com/topics.mbox");
    assert_eq!(display_name(path), "my-group@googlegroups.com");
    assert_ne!(display_name(path), "topics.mbox");

    let es = Path::new("/tmp/Takeout/Grupos/googlegroups.com/mi-grupo@googlegroups.com/temas.mbox");
    assert_eq!(display_name(es), "mi-grupo@googlegroups.com");

    // Merging several groups keeps them distinguishable.
    let names = unique_display_names(&[path.to_path_buf(), es.to_path_buf()]);
    assert_eq!(
        names,
        vec!["my-group@googlegroups.com", "mi-grupo@googlegroups.com"]
    );
}
