# Changelog

All notable changes to mboxshell are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
