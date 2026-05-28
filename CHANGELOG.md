# Changelog

All notable changes to mboxshell are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
