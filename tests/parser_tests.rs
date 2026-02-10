//! Integration tests for the MBOX parser, header decoding, and index system.

use std::path::Path;

use mboxshell::index::builder;
use mboxshell::parser::header::{decode_encoded_words, parse_date};
use mboxshell::parser::mbox::MboxParser;
use mboxshell::store::reader::MboxStore;

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

// ─── Test 1: Parse simple.mbox → exactly 5 messages ─────────────────

#[test]
fn test_parse_simple_mbox_count() {
    let parser = MboxParser::new(fixture("simple.mbox")).unwrap();
    let mut count: u64 = 0;
    parser
        .parse(
            &mut |_offset, _bytes| {
                count += 1;
                true
            },
            None,
        )
        .unwrap();
    assert_eq!(count, 5, "simple.mbox should contain exactly 5 messages");
}

// ─── Test 2: First message fields ───────────────────────────────────

#[test]
fn test_parse_simple_mbox_first_message() {
    let entries = builder::build_index(&fixture("simple.mbox"), true, None).unwrap();
    assert!(!entries.is_empty());
    let first = &entries[0];
    assert_eq!(first.subject, "Hello World");
    assert_eq!(first.from.address, "user1@example.com");
    assert_eq!(first.from.display_name, "User One");
    assert_eq!(first.message_id, "<msg001@example.com>");
}

// ─── Test 3: Encoded words in From and Subject ──────────────────────

#[test]
fn test_parse_encoded_words() {
    let entries = builder::build_index(&fixture("simple.mbox"), true, None).unwrap();
    assert!(entries.len() >= 3);
    let third = &entries[2];
    // From: =?UTF-8?B?Sm9zw6kgR2FyY8Ota2E=?= → "José Garcíka"
    assert!(
        third.from.display_name.contains("Jos"),
        "Expected decoded From display name, got: '{}'",
        third.from.display_name
    );
    // Subject: =?UTF-8?Q?Caf=C3=A9_con_le=C3=B1a?= → "Café con leña"
    assert!(
        third.subject.contains("Caf"),
        "Expected decoded subject, got: '{}'",
        third.subject
    );
    assert!(
        third.subject.contains("le"),
        "Expected decoded subject with 'le', got: '{}'",
        third.subject
    );
}

// ─── Test 4: >From in body is not a separator ───────────────────────

#[test]
fn test_from_escaping_in_body() {
    // The fourth message has ">From " in its body.
    // This should NOT split it into two messages.
    let entries = builder::build_index(&fixture("simple.mbox"), true, None).unwrap();
    assert_eq!(
        entries.len(),
        5,
        "Should still be 5 messages (>From not a separator)"
    );

    let fourth = &entries[3];
    assert_eq!(fourth.subject, "Message with From in body");

    // Verify the body contains the >From line
    let mut store = MboxStore::open(fixture("simple.mbox")).unwrap();
    let body = store.get_message(fourth).unwrap();
    let text = body.text.as_deref().unwrap_or("");
    assert!(
        text.contains("From the perspective") || text.contains(">From the perspective"),
        "Body should contain the >From line, got: '{}'",
        text
    );
}

// ─── Test 5: Empty MBOX → 0 messages, no error ─────────────────────

#[test]
fn test_parse_empty_mbox() {
    let parser = MboxParser::new(fixture("empty.mbox")).unwrap();
    let mut count: u64 = 0;
    let result = parser.parse(
        &mut |_offset, _bytes| {
            count += 1;
            true
        },
        None,
    );
    assert!(result.is_ok());
    assert_eq!(count, 0);
}

// ─── Test 6: Index build and reload ─────────────────────────────────

#[test]
fn test_index_build_and_reload() {
    let tmp = tempfile::tempdir().unwrap();
    let mbox_path = tmp.path().join("test.mbox");
    std::fs::copy(fixture("simple.mbox"), &mbox_path).unwrap();

    // Build
    let entries = builder::build_index(&mbox_path, true, None).unwrap();
    assert_eq!(entries.len(), 5);

    // Verify index file exists
    let idx_path = builder::index_path_for(&mbox_path);
    assert!(
        idx_path.exists(),
        "Index file should exist at {:?}",
        idx_path
    );

    // Reload
    let loaded = builder::load_index(&mbox_path).unwrap();
    assert!(loaded.is_some(), "Should be able to reload the index");
    let loaded = loaded.unwrap();
    assert_eq!(loaded.len(), 5);

    // Verify data matches
    assert_eq!(loaded[0].subject, entries[0].subject);
    assert_eq!(loaded[0].from.address, entries[0].from.address);
    assert_eq!(loaded[4].message_id, entries[4].message_id);
}

// ─── Test 7: Index invalidation on file change ─────────────────────

#[test]
fn test_index_invalidation() {
    let tmp = tempfile::tempdir().unwrap();
    let mbox_path = tmp.path().join("test.mbox");
    std::fs::copy(fixture("simple.mbox"), &mbox_path).unwrap();

    // Build index
    let _ = builder::build_index(&mbox_path, true, None).unwrap();

    // Wait a moment so the mtime changes
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Modify the MBOX by appending content
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&mbox_path)
        .unwrap();
    writeln!(f).unwrap();
    writeln!(f, "From extra@example.com Mon Jan 08 10:00:00 2024").unwrap();
    writeln!(f, "From: Extra <extra@example.com>").unwrap();
    writeln!(f, "Subject: Extra message").unwrap();
    writeln!(f, "Date: Mon, 08 Jan 2024 10:00:00 +0000").unwrap();
    writeln!(f, "Message-ID: <msg006@example.com>").unwrap();
    writeln!(f).unwrap();
    writeln!(f, "Extra body.").unwrap();
    drop(f);

    // Try to load — should return None (invalid)
    let loaded = builder::load_index(&mbox_path).unwrap();
    assert!(
        loaded.is_none(),
        "Index should be invalidated after file modification"
    );
}

// ─── Test 8: Charset decoding ───────────────────────────────────────

#[test]
fn test_charset_decoding() {
    let entries = builder::build_index(&fixture("encoded_words.mbox"), true, None).unwrap();
    assert_eq!(entries.len(), 3);

    // ISO-8859-1: François, Résumé du projet
    assert!(
        entries[0].from.display_name.contains("Fran"),
        "Expected François, got: '{}'",
        entries[0].from.display_name
    );
    assert!(
        entries[0].subject.contains("sum"),
        "Expected Résumé du projet, got: '{}'",
        entries[0].subject
    );

    // UTF-8: 山田太郎, お知らせ
    assert!(
        entries[1].from.display_name.contains("山田"),
        "Expected 山田太郎, got: '{}'",
        entries[1].from.display_name
    );

    // Windows-1252: Müller
    assert!(
        entries[2].from.display_name.contains("ller"),
        "Expected Müller, got: '{}'",
        entries[2].from.display_name
    );
}

// ─── Test 9: Read message by offset ─────────────────────────────────

#[test]
fn test_read_message_by_offset() {
    let entries = builder::build_index(&fixture("simple.mbox"), true, None).unwrap();
    assert!(entries.len() >= 3);

    let mut store = MboxStore::open(fixture("simple.mbox")).unwrap();
    let body = store.get_message(&entries[2]).unwrap();
    let text = body.text.as_deref().unwrap_or("");
    assert!(
        text.contains("áéíóú") || text.contains("especiales"),
        "Third message body should contain Spanish characters, got: '{}'",
        text
    );
}

// ─── Test 10: Parse single EML ──────────────────────────────────────

#[test]
fn test_parse_single_eml() {
    let entry = mboxshell::parser::eml::parse_eml(fixture("single.eml"), 0).unwrap();
    assert_eq!(entry.subject, "Single EML Test");
    assert_eq!(entry.from.address, "sender@example.com");
    assert_eq!(entry.from.display_name, "Test Sender");
    assert_eq!(entry.message_id, "<eml001@example.com>");
}

// ─── Test 11: Date parsing in multiple formats ──────────────────────

#[test]
fn test_date_parsing_formats() {
    // RFC 2822 with day-of-week
    let d1 = parse_date("Thu, 04 Jan 2024 10:00:00 +0000");
    assert!(d1.is_some(), "Failed to parse RFC 2822 date");

    // Without day-of-week
    let d2 = parse_date("04 Jan 2024 10:00:00 +0000");
    assert!(d2.is_some(), "Failed to parse date without day-of-week");

    // Named timezone
    let d3 = parse_date("Thu, 04 Jan 2024 10:00:00 EST");
    assert!(d3.is_some(), "Failed to parse date with named timezone");

    // ISO 8601
    let d4 = parse_date("2024-01-04T10:00:00Z");
    assert!(d4.is_some(), "Failed to parse ISO 8601 date");
}

// ─── Test 12: In-Reply-To and References ────────────────────────────

#[test]
fn test_threading_headers() {
    let entries = builder::build_index(&fixture("simple.mbox"), true, None).unwrap();
    assert!(entries.len() >= 2);

    let second = &entries[1];
    assert_eq!(second.subject, "Re: Hello World");
    assert_eq!(
        second.in_reply_to.as_deref(),
        Some("<msg001@example.com>"),
        "in_reply_to should be <msg001@example.com>"
    );
    assert!(
        second
            .references
            .contains(&"<msg001@example.com>".to_string()),
        "references should contain <msg001@example.com>, got: {:?}",
        second.references
    );
}

// ─── Encoded-words unit tests ───────────────────────────────────────

#[test]
fn test_decode_encoded_words_base64_utf8() {
    assert_eq!(
        decode_encoded_words("=?UTF-8?B?SG9sYSBtdW5kbw==?="),
        "Hola mundo"
    );
}

#[test]
fn test_decode_encoded_words_q_iso8859() {
    assert_eq!(decode_encoded_words("=?ISO-8859-1?Q?caf=E9?="), "café");
}

#[test]
fn test_decode_encoded_words_plain_passthrough() {
    assert_eq!(decode_encoded_words("Normal subject"), "Normal subject");
}
