# Changelog

All notable changes to mboxshell are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## v0.8.0

A security, accessibility and usability release from a full review of the tool, plus Maildir export and a light theme. It also carries the v0.7.4 hardening below, which was never published on its own. 29 new tests (290 total). No index format change.

**Security and data safety**

- Fix: **the HTML sanitizer had two known XSS holes.** ammonia 4.1.2 → 4.2.0 (RUSTSEC-2026-0193, mXSS through MathML; RUSTSEC-2026-0213, SVG `animate`/`set`). It is the only thing between a hostile email and the browser in HTML exports and the `H` view. anyhow, lru and crossbeam-epoch were updated for their advisories too, and CI now runs `cargo audit`.
- Fix: **the per-line memory cap of the indexer did not cap anything.** `read_until` loaded the whole physical line before truncating it, so an 800 MB mailbox without line breaks used 816 MB of RAM to index; it now uses about 20 MB, whatever the file.
- Fix: **a deep reply chain crashed the threaded view.** Flattening a thread recursed once per level, and a mailbox whose `References` chain thousands of messages overflowed the stack — an abort that left the terminal in raw mode. It is now iterative.
- Fix: **the `.mboxshell.idx` index, the CSV export and the merge/export temp files were opened through whatever sat at their path.** A mailbox shipped with a planted `.name.mboxshell.idx -> ~/.zshrc` symlink got that file overwritten on first open. They are now written to a fresh, uniquely named temp file (`create_new`) and renamed into place, which replaces a link instead of writing through it.
- Fix: **`stats` printed sender names raw**, so an encoded `From:` could send escape sequences to the terminal (retitle it, write the clipboard through OSC 52). They are sanitized like the rest of the output.
- Change: **remote images are blocked in HTML exports and the `H` view.** Opening the page would fetch them, and a tracking pixel tells the sender when and from where the archive was read. A note at the top of the page says how many were blocked; `export --allow-remote-images` keeps them.
- Fix: exported and extracted file names that are reserved devices on Windows (`CON`, `NUL`, `COM1`…) get a `_` prefix; a message with more than 999 same-named attachments no longer overwrites them into one `_dup` file; the HTML-viewer temp file is created `0600`.
- Change: **`merge` joins inputs with the blank line an mbox needs, and only where it is missing.** v0.7.4 added a single newline, which still left the next `From ` line right after a non-blank one. Well-formed inputs are still concatenated byte for byte.
- CI: every GitHub Action is pinned to a commit SHA, and the release `build` job, which runs third-party build scripts, is read-only; only the job that publishes can write.

**Input robustness**

- Fix: **a search filter that could not be read was silently dropped.** `after:2099-13-45`, `date:foo`, `size:big` or `has:xyz` returned *everything*, and `export --query` then exported the whole mailbox. The search now fails with a clear message (exit code 1 on the CLI, the status bar in the TUI).
- Fix: **a file that is not a mailbox was accepted as "0 messages" with success** and left a hidden index behind. A non-empty file with no `From ` separator is now rejected as not an MBOX mailbox.
- Fix: **selecting a huge message allocated all of it.** Displaying or searching a message reads at most 256 MB of it, with a notice at the top; exports still copy every byte.
- Fix: log warnings were written to stderr in colour even when redirected, and on top of the TUI screen; they now carry colour only on a terminal and go to the log file only while the TUI is open.

**Accessibility**

- Feature: **the `theme` setting works** (#30). `light` is new, on its own light background so it reads the same on a dark terminal, with every text colour at WCAG AA (4.5:1) or better; `terminal` uses no colours at all and follows the terminal's own palette. `NO_COLOR` forces `terminal`, and `MBOXSHELL_THEME` overrides the config file. `dark` stays the default, with its dimmer texts raised to 4.5:1.
- Feature: **the selected message is marked with `>`**, the active sidebar filter with `•`, so neither depends on colour.
- Feature: **the terminal's real cursor follows the focus** — the selected row, the text being typed, the active popup option — so screen readers, braille displays and magnifiers can track it.
- Fix: **Ctrl-C quits from anywhere**; inside a search prompt it typed a `c`. Text fields ignore Ctrl/Alt chords (AltGr still types), and Ctrl-U clears them.
- Fix: the help popup scrolls (it was cut off at 80x24), the status bar never drops `?` and `q`, and errors and export results stay until the next key press instead of vanishing after 5 seconds.
- Fix: opening the TUI without a terminal (a pipe, `TERM=dumb`) now says so and points at `stats` / `search --json`.
- Fix: the help popup panicked in Spanish on terminals 21–38 columns wide (it cut "página" in the middle of a character).

**Features and polish**

- Feature: **`export --format maildir`** (#29) writes a standard Maildir (`cur/`, `new/`, `tmp/`), works with `--query`, and turns `Status:`/`X-Status:` — and Gmail's `Opened`/`Starred` labels — into Maildir flags.
- Change: `--format` is validated before indexing and `--help` lists its values; `--help` is fully translated to Spanish; running without arguments exits with 2, and `merge` needs at least two inputs.
- Fix: I/O errors are translated and no longer print their cause twice; the remaining hard-coded English in the TUI is translated; the manuals no longer claim that `.eml` files and folders can be opened.

## v0.7.4

Never published on its own: these changes shipped in v0.8.0, which refines two of them (the merge now joins inputs with a blank line, and temp files are uniquely named rather than `.mbox.tmp`).

Security and correctness hardening of merge, export, CSV and threading. 20 new tests (261 total).

- Fix: **merge deduplication could be poisoned to make a legitimate message disappear.** Two messages were duplicates as soon as they shared a Message-ID, so a message planted in an earlier input with the Message-ID of a legitimate one silently removed the legitimate one from the merged archive. A message is now a duplicate only when the Message-ID **and** a SHA-256 of its content match an earlier one; the content hash leaves out the `From ` envelope line (it records the delivery into *that* mailbox, so the same email exported twice legitimately differs there) and trailing line breaks. Messages without a Message-ID are still never deduplicated. A consequence: two copies of one email whose headers differ (for example different `X-Gmail-Labels` in two Takeout exports) are now both kept.
- Fix: **`merge --no-dedup` lost the first message of an input when the previous input did not end in a newline.** Its last line swallowed the next `From ` line, so the merge reported N messages while the output re-indexed as fewer. A newline is now inserted between inputs in that case (and the dedup/source-header path keeps inputs apart the same way).
- Fix: **`merge --no-dedup` read every input whole into memory**, which for the multi-GB mailboxes this tool targets meant multi-GB of RAM. Inputs are now streamed in 1 MiB blocks, counting `From ` lines at line start with the state carried across blocks. `memchr`, already in the dependency tree through `serde_json`, is now a direct dependency.
- Fix: **`export --format mbox` and `merge` refuse to write over one of their own source mailboxes** (the destination, or its `.mbox.tmp`, being the same file as an input: same device and inode on Unix, so symlinks and hard links are caught; same canonical path elsewhere). Before, the final rename replaced the source mailbox with the export. New `MboxStore::path()`.
- Fix: **a failed export or merge no longer leaves its `.mbox.tmp` behind.**
- Fix: **CSV export: cells containing `;` are now quoted**, since spreadsheets in locales that use the comma as decimal separator (Excel in es-ES, de-DE, fr-FR…) split fields on `;`. The To, CC and Labels columns formula-guard every item before joining them with `; `, so no fragment after a `;` can start with `=`, `+`, `-` or `@`; the guard also looks past leading spaces.
- Fix: **threading: a crafted Message-ID could hide a message from the threaded view.** The keys invented for messages without a Message-ID (`__synth_N__`) and for the second copy of a repeated one (`__dup_N__`) were valid Message-IDs that a message could carry. They now start with NUL, which is stripped from every real id.
- Fix: **attachments never carried their `Content-ID`** (`AttachmentMeta.content_id` was always `None`), so inline `cid:` images could not be resolved by anything built on the attachment list. It is now read from the part's `Content-ID` header, without angle brackets.

## v0.7.3

The Mac app moved to a new domain.

- Docs: **mboxViewer now lives at `mboxviewerpro.com`.** Both READMEs and both manuals still pointed at `mboxviewer.net`, the macOS app's former domain. The repository's own project link points at the new domain too.

## v0.7.2

Export a selection back out as a new mailbox. 7 new tests (241 total).

- Feature: **`export --format mbox` writes the selected messages to a single new MBOX mailbox.** Until now a selection could be exported as `.eml` / CSV / text / HTML — a folder of files — but never back as a mailbox, so there was no way to hand over *part* of an archive in the format it came in. Combined with `--query` this is the delivery flow: filter a large export (a Google Vault dump, a Takeout archive) down to the messages that actually belong in a handover — a legal request, a records request, a mailbox someone else has to read — and produce a mailbox containing only those. A message read out of an MBOX is copied byte for byte, keeping its own `From ` envelope line and whatever quoting the source used, because rewriting an envelope could only corrupt an archive that was already valid; a message with no envelope line (one that came from an EML) gets one synthesized — C `asctime` in UTC with the day space-padded to two columns, locale-independent — plus `>`-quoting of body lines starting with `From `, without which the result is not a readable mailbox. Every record ends in a newline. The output is committed by temp-file-and-rename, like the merge, so a mid-export failure never leaves a half-written mailbox under the name the user asked for; the source file is never touched. New `export::mbox::export_mbox` and `export::mbox::mbox_record`.

## v0.7.1

Search semantics: `OR` gains a precedence, and repeated date/size filters stop overwriting each other. 23 new tests (234 total).

- Fix: **`OR` had no precedence — a single `OR` turned every term in the query into an alternative.** `from:alice OR from:bob subject:invoice` did not mean "either sender, about invoices": it returned everything from alice, everything from bob, *and* everything whose subject said invoice, whoever sent it. On a 6,787-message mailbox that is the difference between 49 hits and 2,102. A query is now a list of AND-ed groups, each group a list of OR-ed terms, and `OR` is what puts two terms in the same group — `a OR b c` reads as `(a OR b) AND c`. There are still no parentheses, which is what the syntax always implied. `SearchQuery.terms`/`is_or` give way to `groups` (with `all_terms()` for callers that only need what was parsed), and the full-text pass follows the same shape: it defers whole groups and settles each one after reading the body, so `body:x OR subject:y` behaves. Metadata terms inside a deferred group are judged by the metadata matcher instead of counting as an automatic match.
- Fix: **`after:X before:Y` was impossible to express.** The parsed query held a single optional date filter, so the second one silently replaced the first and only `before:` survived — half of what the user typed was dropped without a word. Date and size filters now accumulate and are AND-ed, which also makes `size:>1mb size:<5mb` a band instead of a rewrite.
- Change: **`after:` now includes its own day**; `before:` still excludes its own. So `after:2024-01-01 before:2025-01-01` is exactly the year 2024 — the half-open reading Gmail's operators use, and consistent with the already-inclusive `date:A..B`. A bare `after:2024-06-15` now returns messages from the 15th itself.
- Docs: both manuals and both READMEs document the precedence rule, the date bounds, and that repeating a filter narrows instead of replacing.

## v0.7.0

Support for the Google Groups mailboxes a Takeout archive ships alongside the Gmail export. 13 new tests (215 total). **The index format changes, so every existing `.mboxshell.idx` is rebuilt on first open.**

- Feature: **Google Groups mailboxes from Google Takeout are now handled as first-class mailboxes.** A Takeout archive contains two different kinds of mbox, not one: besides the Gmail export, every group the account owns is exported as `Groups/googlegroups.com/<group>@googlegroups.com/topics.mbox`. In the archive this was built against, that was the single largest file of the whole export — 636 MB / 6,787 messages, against 292 MB for the Gmail mbox. Four Groups-specific details are now handled: the envelope date format, the missing labels, the meaningless file name, and the explicit conversation id. See [`docs/GOOGLE-GROUPS.md`](docs/GOOGLE-GROUPS.md) for the layout, the measured header coverage and the exact rules.
- Fix: **the `From ` envelope date of a Groups message no longer resolves to an invented date.** Google Groups writes the timezone offset *before* the year (`Thu Apr 16 09:53:04 +0000 2015`), which asctime does not allow, so none of the known formats matched and a message with no parseable `Date:` header fell through to a last-resort parse that returned a plausible-looking but wrong `2000-09-16` — no error, just a corrupted sort order and date filters. The format is now recognised, after plain asctime, which it does not shadow.
- Feature: **the group is surfaced as a virtual label**, so the sidebar works for a whole Takeout archive instead of its Gmail half. Taken from `X-Google-Groups`, falling back to `X-BeenThere` restricted to `googlegroups.com` (it is a generic mailing-list header that Mailman writes too). It is *added* to `X-Gmail-Labels` rather than replacing it: in a Groups mailbox that header holds localised topic state (`El tema se ha fijado`, `Las respuestas del tema están bloqueadas`) worth keeping.
- Fix: **a Groups mailbox is no longer displayed as `topics`.** The file is always called `topics.mbox` — `temas.mbox` and so on in every other locale — and what identifies it is the parent directory `<group>@googlegroups.com`. `presentation_path` now resolves it, the same way it already resolved Apple Mail's `Inbox.mbox/mbox` package, so the group name is what the TUI shows and what `merge --source-header` records.
- Fix: **the TUI header bar showed the raw file name.** It called `file_name()` directly instead of resolving the mailbox name, so it displayed the localised `temas.mbox` for a Groups export and a bare `mbox` for an Apple Mail store.
- Feature: **conversations are threaded by `X-GM-THRID` when the mailbox provides one.** Gmail and Groups exports carry an explicit server-assigned conversation id. The JWZ reference tree is unchanged; the id only replaces the subject heuristic in the final step that merges root containers, where merging by normalized subject both over-merges (every conversation titled "Hello" becomes one thread) and under-merges (a reply whose subject was edited splits off). On the reference mailbox: 1,927 → 1,958 threads, and the largest thread shed 7 unrelated messages (73 → 66).
- Fix: **a message whose `Message-ID` is repeated no longer disappears from the threaded view.** The later copy took over the container the first one had claimed, whose index was then referenced by nothing — the reference mailbox showed 6,785 of its 6,787 messages in thread mode. The later copy now gets an id of its own and still reaches its conversation through its references.
- Fix: **`-f` was claimed by two different options at once.** `--force` was declared global with short `-f`, which `export` also used for `--format`. Debug builds panicked at startup on `export` (`cargo run -- export …` was unusable) and release builds silently gave `-f` to `--format`. `force` is now declared per subcommand, so `-f` keeps meaning `--force` everywhere except `export`, where it means `--format` and `--force` is spelled out in full. Both positions still work: `mboxshell -f index x.mbox` and `mboxshell index x.mbox -f`.
- Change: **index format version 3 → 4; existing indexes are rebuilt automatically on first open.** `MailEntry` gained the `thread_id` field. Since that re-index is forced anyway, this release also lands the mailbox-mtime change that had been deliberately deferred until an intentional format bump: the index now records the source mtime in nanoseconds instead of seconds, closing the sub-second window in which a mailbox rewritten within the same second as its index looked unchanged and the stale index was served.

## v0.6.2

Fixes the source-mailbox header on Apple Mail exports, and documents the flag that shipped undocumented in v0.6.1. 7 new tests (202 total).

- Fix: **`merge --source-header` now writes the mailbox name the user knows, not `mbox`.** Apple Mail exports a mailbox as a *directory* `Inbox.mbox` containing a file called literally `mbox`, which is the path actually read — so `X-Mbox-Source` was stamped `mbox` on every message of every such mailbox, making a merged archive untraceable, which is the one thing the header exists for. The label is now taken from the containing `.mbox` package (`Inbox.mbox`), while a file named `mbox` that is *not* inside a package keeps its own name. Mailboxes that would end up sharing a label are disambiguated against each other by prepending parent directories (`Work/Inbox.mbox` vs `Personal/Inbox.mbox`), up to 4 levels, stopping when a name can no longer grow so identical inputs terminate instead of climbing to the filesystem root. The same name is passed to the progress callback. New `mailbox_naming` module (`presentation_path` / `display_name` / `unique_display_names`); the label is still sanitized before it reaches the header.
- Fix: **the docs described a `merge --dedup` flag that does not exist.** Deduplication is on by default and the real flag is `--no-dedup`; both READMEs and both manuals said otherwise, so anyone copying the documented command got an error. `--source-header` shipped in v0.6.1 without any documentation at all — it is now covered in the READMEs and manuals, with the Apple Mail naming rule spelled out.
- Fix: the merge summary line `Tagged with source` was hardcoded in English while every other line went through the catalogue — it is now translated (`Etiquetados con origen`).
- Chore: restored a clean `cargo fmt --check` and `cargo clippy -D warnings` under the current stable toolchain, whose style and lints (`clippy::question_mark`) had begun rejecting code already on `main`.

## v0.6.1

Merge gains an optional source-mailbox header. 6 new tests.

- Feature: **`merge --source-header` injects an `X-Mbox-Source: <origin file name>` header into every merged message**, so a combined archive stays traceable back to which mailbox each email came from. The header is inserted right after the `From ` envelope line (becoming the message's first real header), reuses the message's own line terminator (CRLF vs LF), preserves a leading UTF-8 BOM, and is prepended when a message has no envelope line. The origin file name is sanitized (control chars incl. CR/LF stripped) so a crafted file name can't inject extra headers. Enabling it forces the per-message merge path (it needs message boundaries) instead of the fast raw block copy; a plain merge is unchanged. `merge_mbox_files` takes a new `add_source_header` parameter and `MergeStats` reports `source_header_added`.

## v0.6.0

Interactive-performance release for large mailboxes (the 50GB / ~500k-message files this project targets). No change to what the TUI shows — only how much work it does per keystroke and per frame. 6 new tests.

- Performance: **the message view no longer rebuilds and re-wraps the whole body on every frame.** Rendering a message used to re-sanitize and re-style every line and word-wrap the entire body twice per frame — thousands of allocations for a large HTML mail, and it drew ~10 times a second even while idle. The styled lines are now cached and reused until something that affects them changes (the selected message, the panel width, the raw / full-headers toggles, or the in-body search state), so idle frames and scrolling skip the rebuild and one of the two wraps.
- Performance: **incremental search is debounced and does a single pass.** Every keystroke in the search bar used to scan all messages twice (plus build a hash set) synchronously, so typing lagged on large mailboxes. It now runs one scan, and only after typing settles (~150 ms), so a burst of keystrokes coalesces into a single search instead of one search per key. Pressing Enter still runs the full (including body) search immediately.
- Performance: **selecting a message no longer deep-copies its decoded body.** Moving the cursor used to clone the whole decoded message — text, HTML, headers and attachments, potentially several MB — out of the cache on every arrow-key press. Bodies are now shared by reference, so selection is a cheap reference-count bump.

## v0.5.3

- Fix: **the "mark all" (`*`) shortcut now toggles the visible rows, not a global count.** It compared `marked.len()` against the visible count, so with a filter active — when the number of globally-marked messages happened to equal the visible count — it could clear everything (or mark the wrong set). It now marks the visible rows, unmarks them if all are already marked, and never touches marks outside the current view.
- Fix: **a message with no blank line before the next `From ` separator is no longer silently dropped from the index.** Such a (malformed) message never had its headers finalized, so the indexer skipped it; it is now emitted from the accumulating buffer.
- Fix: **the header/body split is detected correctly on messages with mixed line endings.** A message with CRLF headers but an LF-only blank line early in the body could pull body text into the raw-header view (and vice-versa); the boundary is now taken as the earliest of `\n\n` and `\r\n\r\n`.
- Fix: **subject normalization for threading is now linear instead of O(n²)** — it used to re-lowercase the whole subject for each stripped `Re:`/`Fwd:` prefix — and no longer risks panicking on a non-ASCII subject.
- Fix: the index's 4 KB content hash is now read deterministically (a short `read` could hash a different prefix length between build and load and trigger a spurious rebuild), and the internal `read_message_at` uses a checked length conversion instead of a cast that could truncate on 32-bit targets.

## v0.5.2

- Security: **the `search` and `stats` CLI tables now sanitize message header fields before printing them.** A hostile subject or sender could carry ESC/OSC terminal escape sequences (and RFC 2047 encoded-words decode into real control bytes), which were written straight to the terminal — the same terminal-injection class the TUI already guarded against. The CLI now runs those fields through the same sanitizer; the `--json` output was already safe.
- Security: **the quoted-printable re-encoder (`export --qp`) is now depth-limited.** A deeply nested `multipart/*` message could recurse without bound and overflow the stack; nesting beyond 32 levels now emits the inner part unchanged.
- Security: **a `.mboxshell.idx` sidecar that is implausibly large for its mailbox is now rejected before being read into memory**, so a crafted index can't force a huge allocation ahead of validation.
- Dependencies: **bumped ratatui 0.29 → 0.30 and lru 0.16 → 0.18.** This removes the vulnerable `lru 0.12.5` that ratatui pulled in transitively (GHSA-rhfx-m35p-ff5j, a low-severity `IterMut` Stacked-Borrows issue); the dependency tree now resolves a single `lru 0.18`.

## v0.5.1

- Fix: **exporting with several messages marked now writes every marked message for HTML and TXT, not just the current one.** EML and CSV export already honored the marked set, but HTML and TXT only wrote the focused message, so exporting a marked selection produced a single file. Both now write one file per marked message, all at once. Thanks to @nekromoff (#20).
- Fix: **messages with no parseable `Date:` header now fall back to the `From ` separator's envelope date instead of 1970.** Gmail Takeout chats, drafts and many automated messages carry no `Date:` header; they were all stamped `1970-01-01` and clumped together at one end of every date sort. The mbox `From_` line's asctime date is now used as the documented fallback.
- Fix: **the named timezones CET, CEST and JST are no longer read as UTC.** chrono's RFC 2822 parser only recognizes the North-American obs-zones (EST/EDT/…/PST/PDT) and treated other alphabetic zones as `-0000`, so European/Asian mail was off by 1–9 hours — enough to shift the calendar day near midnight. Named zones are now substituted with their numeric offset before parsing, and the substitution table is ordered so `CEST` is no longer swallowed by the `EST` rule.
- Docs: removed the READMEs' claim of `.eml` and EML-directory input (only MBOX is supported today), corrected a CONTRIBUTING reference to a removed `memmap2` unsafe wrapper (the codebase contains no `unsafe`), and refreshed the user manuals' version banner and the `stats` field list (duplicate `Message-ID` count).

## v0.5.0

Hardening release from a full security and stability audit of the whole codebase. No functional changes to normal use; 13 new regression tests.

- Security: **the external HTML viewer no longer receives unsanitized message HTML.** Opening a message's HTML part in the viewer configured via `MBOXSHELL_HTML_VIEWER` used to write the raw email HTML to a temp file — with a real browser configured as the viewer, hostile `<script>` / `javascript:` markup from an email could execute. The HTML is now passed through the same `ammonia` sanitizer used by HTML export before being written, and the temp file is created with exclusive-create semantics so it cannot follow a pre-planted symlink on a shared `/tmp`.
- Security: **the streaming indexer can no longer be driven to run out of memory by a crafted or corrupt mailbox.** Per-line and per-header accumulation during indexing is now bounded: retained bytes are capped while the full physical byte count is still consumed, so message offsets stay exact. A message with an unwrapped multi-gigabyte line, or with no blank line before the next `From ` separator, is truncated in memory instead of buffering without limit.
- Fix: **several inputs that used to panic are now handled safely.** Exporting a message whose subject is a long non-ASCII string (CJK, accents, emoji) sliced the generated filename in the middle of a UTF-8 character and crashed; a Gmail label wider than the sidebar did the same on every render frame. Filenames and labels now truncate on character boundaries. A `size:` search filter with an oversized value (e.g. `size:>99999999999gb`) overflowed and panicked in debug / wrapped to a wrong threshold in release; it now cancels the filter instead. A narrow terminal could underflow the header-bar padding arithmetic. And a panic hook now restores the terminal (leaves raw mode and the alternate screen) before unwinding, so a crash no longer leaves the shell unusable.
- Fix: **attachments are extracted by their position in the message, not by matching filename.** A message with several parts sharing one filename (e.g. three inline `image.png`) previously exported the *first* part's bytes for all of them — silent data corruption on "save all attachments" — and an attachment with no filename could not be extracted at all. Each part now records its index and is located by it.
- Fix: **the byte-exact MBOX merge no longer corrupts its output, and `--no-dedup` is reachable.** The non-dedup merge path read the input as UTF-8 lines, which aborted on any 8-bit byte (routine in real mail) and rewrote CRLF to LF; it now copies raw bytes verbatim. The merge writes to a temp file and renames it on success, so an error mid-merge never leaves a half-written output. `mboxshell merge` gained a `--no-dedup` flag — the byte-exact path was previously unreachable because the old `--dedup` boolean defaulted to true and could not be turned off. Single-message EML / HTML / TXT exports no longer silently overwrite one another on a filename collision; the second file gets a `_1` suffix, matching the attachment exporter.

## v0.4.7

- Fix: **stale on-screen artifacts in the TUI caused by control characters in message text.** Raw control characters in a message (tabs were the typical culprit, in bodies and even headers) were written to the terminal as-is; the terminal moved the cursor on its own, desyncing ratatui's cell-diffing from what was actually on screen, so fragments of previously viewed messages survived in areas the UI believed blank. All rendered text — body, raw view, headers and list columns — is now sanitized before drawing: tabs expand to 8-column tab stops and other control characters show as a visible `�`. In-body search offsets were adjusted to match, so match highlighting stays aligned. Thanks to @jpetrina for the report and for verifying the fix (#17).

## v0.4.6

- Fix: **emails quoted verbatim inside a message body no longer split the containing message.** A `From `-prefixed line now separates messages only when it is structurally a real mbox separator — `From <sender> <asctime date>` ending right after the date (optional timezone allowed), or a bare `From ` line with nothing after it (as Thunderbird writes when exporting a Gmail account). Quoted `git format-patch` mails — whose pseudo-separator carries git's fixed magic date `Mon Sep 17 00:00:00 2001` — are treated as content inside a normal mailbox, while a file that *starts* with one (a patch series, itself valid mbox) still splits on every patch. The index format version was bumped, so existing indexes are rebuilt automatically with the corrected boundaries. Thanks to @jpetrina for the report, the on-the-spot debugging and the bare-separator fix (#16).
- Change: **metadata search and column sorting allocate far less memory.** Case-insensitive matching no longer lowercases every field of every entry per search term (ASCII fields are compared in place; non-ASCII keeps full Unicode folding), and sorting by From/Subject computes each row's key once instead of twice per comparison — both noticeable on mailboxes with hundreds of thousands of messages.

## v0.4.5

- Security: **a corrupt or crafted index file can no longer trigger a huge memory allocation.** When loading a `.mboxshell.idx`, the `offset + length` of every entry is now validated (with overflow-safe arithmetic) against the actual MBOX size; an index with out-of-range entries is treated as invalid and rebuilt, just like any other stale index. Message lengths are also converted with a checked conversion in the store instead of a silent cast that could truncate on 32-bit targets.
- Security: **CSV exports now guard against spreadsheet formula injection.** Field values starting with `=`, `+`, `-`, `@`, tab or CR get a leading `'` so Excel/LibreOffice display them as text instead of evaluating them as formulas — a subject line like `=cmd|...` in a spam message was a typical vector when opening the exported CSV in a spreadsheet.
- Change: **release binaries are now smaller and slightly faster** — built with thin LTO, a single codegen unit and stripped symbols via a new `[profile.release]` section.
- Change: CI/build housekeeping — cargo caching in CI with superseded runs auto-cancelled, fmt/clippy run once on Linux instead of on every OS, workflow token restricted to read-only, `cross` installed as a pinned prebuilt binary instead of compiled from git HEAD, builds with `--locked`, and the unused `memmap2` and `byteorder` dependencies removed.

## v0.4.4

- Fix: **header lines that land exactly on the 1 MB read-buffer boundary are no longer split**, which previously truncated a message's headers and dropped everything after the split point (`Subject`, `Date`, …). Affected messages showed up with a `1970-01-01` date and an `unknown` subject, and inflated the message count. The line reader now accumulates a full physical line across buffer refills instead of treating a partial chunk as a complete line. Thanks to @jpetrina for the precise diagnosis and proposed fix (#15).

## v0.4.3

- Add: the `stats` command now reports a **`Duplicates` line** counting messages that repeat a `Message-ID` already seen, alongside the number of distinct IDs — e.g. `Duplicates  185 (42 Unique IDs)`. Messages without a `Message-ID` are not counted as duplicates. The same `duplicates` / `unique_ids` figures are included in `stats --json`. Thanks to @jpetrina (#14).
- Change: the **message body and header values now render in a brighter near-white** (`rgb(235,235,245)` instead of `rgb(220,220,230)`), for better contrast on black backgrounds — especially under translucent terminals where the previous shade could look dim (#13).

## v0.4.2

- Fix: in-body search `n` / `N` now **reliably scrolls the focused match into view**. The auto-scroll measured position in *unwrapped* lines while the body actually scrolls over *wrapped* rows, so on messages with long lines the match could land off-screen and `n`/`N` appeared to do nothing. Scrolling is now wrap-aware (it uses ratatui's own word-wrap to map a match to its on-screen row), which also lets the body scroll cleanly all the way to the end (#12).
- Change: the in-body search prompt now appears at the **top of the message panel**, right next to the body being searched, instead of in the global bottom bar (#12).
- Add: **vertical keyboard navigation in the Search Filters popup.** `↑` / `↓` move between fields (alongside `Tab` / `Shift-Tab`) and `PgUp` / `PgDn` (or `Home` / `End`) jump to the first / last field. The Size and Label selectors now change their value with `←` / `→` (with `j` / `k` kept as aliases), since the arrow keys now move between fields (#13).

## v0.4.1

- Add: **interactive search within the open message body**, less/vim style. With the message view focused, `/` opens a prompt that highlights every match live as you type; `Enter` confirms and keeps the matches navigable; `n` / `N` jump to the next/previous match with auto-scroll that brings it into view; a `[ current/total ]` counter sits in the body border next to the scroll indicator; `Esc` first clears the matches, then returns to the list. Matching is case-insensitive and Unicode-aware. The global `/` search is unchanged from every other panel (#12).

## v0.4.0

- Add: the message preview now shows a **scroll position indicator** in the bottom-right of its border, so you can tell at a glance whether a body is scrollable and where you are in it — `[ All ]` when everything fits, `[ ↓ Top ]` at the start, `[ ↕ NN% ]` in the middle, and `[ ↑ Bot ]` at the end (#10).
- Fix: **"search within previous results" now works reliably across the full flow.** The toggle was silently reset every time the filter popup reopened, so refining a previous result set with a second field (e.g. a Text/body search after a Subject search) fell back to scanning the whole index. It is now a persistent scoping mode honoured by every search entry point, and is dropped only when the scope itself resets — on a label-filter change or when leaving the search with `Esc` (#11).
- Add: `build_index_cancelable()` — a cancelable variant of index building that polls a `should_cancel` callback per message and aborts without writing a partial index, for embedders that need to interrupt a long indexing run (e.g. the macOS app). `build_index()` is unchanged and fully backward compatible.

## v0.3.8

- Add: `Shift+↑` / `Shift+↓` (and `Shift+PageUp` / `Shift+PageDown`) now scroll the body of the currently selected message in the preview pane without leaving the message list, so you can read a long email while keeping list navigation under the plain arrow keys (#8).
- Change: the standalone status-bar shortcuts are now consistently lowercase — `F:Filters` → `f` and `L:Labels` → `l`. The uppercase `F`/`L` keys still work as hidden aliases, so existing muscle memory keeps working (#9). Shift-paired shortcuts (`s`/`S`, `h`/`H`, `n`/`N`, `a`/`A`, `g`/`G`) are unchanged.

## v0.3.7

- Fix: searches launched from the search bar now respect the active sidebar label filter. When a label was selected, typing a query and pressing `Enter` dropped the scope and matched against every message in the index; the bar now derives a restrict set from the active label and intersects results with it (#7). The empty-query path honours the same scope, so clearing the query no longer escapes the label.

## v0.3.6

- Fix: free-text and `body:`/`filename:` searches no longer freeze the UI. v0.3.5 made the `Text` field scan message bodies, but that scan ran synchronously on the UI thread, so on a large mailbox the whole app locked up until it finished, with no progress and no way to cancel (#6). The body scan now runs on a **background thread**: the interface stays responsive, shows live progress (`Searching message bodies N/M`), and can be cancelled with **Esc**. Metadata-only searches (`from:`, `subject:`, …) still resolve instantly inline.
- Change: a multi-word value in the `Text` field now matches messages that contain **all** the words (AND), searched across subject/from/to **and** the body, instead of looking for that exact contiguous phrase. Field-specific values (`subject:`, `from:`, …) are still treated as quoted phrases.

## v0.3.5

- Fix: the `Text` field in the Search Filters popup (and any free-text/bare-word search) now searches the **message body** in addition to subject/from/to. Previously it only matched header metadata, so a word that lived only in the body returned no results — which made combining `Text` + `Subject` "not always find the match" (#4, #6) and made `Search within previous results` appear broken because its base search returned nothing (#5). As-you-type filtering stays metadata-only and instant; the body scan runs on `Enter`, same cost as an explicit `body:` query. OR queries and field-specific terms are unchanged.

## v0.3.4

- Add: new `Search within previous results` checkbox in the Search Filters popup (`F`). When checked, the new query is intersected with whatever was visible at the moment the popup opened, allowing iterative narrowing of result sets (#5).

## v0.3.3

- Fix: Search Filters popup (`F`) now quotes multi-word values when building the underlying query, so combining `Text` + `Subject` (or any other filter pair where one side has spaces) no longer splits the value across implicit AND terms (#4).
- Fix: quoted phrases in metadata search now use substring matching instead of full-string equality, matching what the fulltext search already did and what users expect from `subject:"monthly report"`-style queries (#4).
- Add: `F: Filters` hint in the mail-list footer so the visual filter popup is discoverable without opening the help screen (#3).

## v0.3.2

- HTML rendering: the built-in message view now uses the `html2text` crate, so tables, lists, headings and links render properly (#1).
- New `H` shortcut: opens the current message's HTML body in an external viewer (configurable via `MBOXSHELL_HTML_VIEWER`, defaults to `w3m`; works with `chawan`, `lynx -dump`, `pandoc`, etc.). The TUI suspends the alternate screen while the viewer runs and restores it cleanly on exit (#1).
- New `html` export format: `mbox-tui export ... --format html` and a new HTML option in the export popup. Produces a standalone HTML page with the headers in a table and the original HTML body (or `<pre>`-wrapped text). **HTML bodies are sanitized by default** (scripts, `on*` handlers, iframes, `javascript:` URLs stripped via the `ammonia` crate); pass `--raw-html` to keep the original markup for local archival (#1).
- Search bar now shows an inline syntax cheatsheet (`from: to: subject: body: date:` …) while empty, so the query language is discoverable without reading docs (#1).
- New `--qp` flag on `export ... --format eml`: re-encodes 8-bit text bodies as quoted-printable so the resulting EML is pure 7-bit ASCII. Helps strict-UTF-8 tools like `eml-extractor` and `emlAnalyzer`. **Works for both single-part and multipart messages** — the MIME tree is walked recursively and every text/* leaf is re-encoded in place (#1).
- CI: bump `actions/checkout`, `actions/upload-artifact` and `actions/download-artifact` to v5 (native Node 24) ahead of GitHub's Sep 2026 Node 20 sunset.

## v0.3.1

- Fix: search bar registered every keystroke and pasted character twice on Windows Terminal and terminals with the kitty keyboard protocol (#2). Key events are now filtered on `KeyEventKind::Press`.
- Fix: in fullscreen layout (`1`), pressing `Tab`/`Enter` on a message now shows the message view full-screen and `Tab`/`Esc` returns to the list (#1). Previously focus moved but nothing visible changed.
- Fix: `.eml` export now reverses mboxrd `>From ` escaping and trims the trailing MBOX separator newline, producing files that are RFC 5322 compliant and accepted by standard parsers (#1).

## v0.3.0

- Search filter popup (`F`): visual form to build queries without remembering syntax (from, to, subject, date range, size, attachment, label).
- Result counter in search bar: shows `(N / total)` while typing.
- Search history: Up/Down arrow keys in the search bar navigate previous queries, with `[history]` indicator.
- New help entries for `F` shortcut and search history hint.
- Complete EN/ES internationalization: all TUI and CLI strings (~150 translation keys), auto-detected from system locale or set with `--lang en|es`.

## v0.2.0

- Incremental search: message list filters as you type (metadata fields only; full-text runs on Enter).
- Dynamic message view title shows current mode: `[RAW]` or `[HEADERS]`.
- Proportional PageDown/Up scroll in message view (adapts to actual viewport height).
- Improved thread indentation with vertical connectors (`│└`) and depth capped at 4 levels.
- Added full CLI commands reference to documentation.

## v0.1.2

- Active panel border highlighted in cyan for clear focus indicator.
- Context-sensitive status bar: hints change depending on the focused panel.
- Version number displayed at the bottom-right corner.
- Help popup reorganized in multi-column layout (adapts to terminal width).
- Help popup now shows app name, version, license and author.

## v0.1.0

- Initial release.
- Streaming MBOX parser (handles 50 GB+ files without loading into memory).
- Persistent binary index for instant re-opens.
- Full terminal UI with vi-style navigation and three layout modes.
- Gmail labels support (X-Gmail-Labels) with sidebar filtering.
- Advanced search: `from:`, `to:`, `subject:`, `body:`, `date:`, `size:`, `has:attachment`, `label:`.
- Conversation threading (JWZ algorithm).
- Export to EML, TXT, CSV with attachment extraction.
- Bilingual interface (English / Spanish).
