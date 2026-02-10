# mboxShell

**Fast terminal viewer for MBOX files of any size. Open, search and export emails from Gmail Takeout backups (50 GB+) without loading them into memory.**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.85%2B-orange.svg)](https://www.rust-lang.org)

[Leer en Espanol / Spanish](README-ES.md)

---

## Why this project exists

When you export your email from Gmail using Google Takeout, you get one or more `.mbox` files that can weigh tens of gigabytes. There is no cross-platform terminal tool that lets you open, search and browse those files efficiently without loading them entirely into memory.

`mboxShell` was built to solve that problem: open a 50 GB MBOX in seconds, navigate hundreds of thousands of messages smoothly, search by sender, date or content, and export whatever you need. All from the terminal, with no GUI, no server, no external dependencies.

## Use cases

- **Browse Gmail backups** (Google Takeout) with their original labels
- **Analyze mail archives** on servers, during migrations or audits
- **Search messages** in MBOX files from any source (Thunderbird, Unix servers, etc.)
- **Export messages** to EML, CSV or plain text for further processing
- **Extract attachments** individually or in bulk
- **Merge multiple MBOX files** into one, removing duplicates

## Features

- **Never loads the file into memory.** Uses streaming I/O with a 1 MB buffer. A 100 GB MBOX uses roughly the same ~500 MB of RAM as a 1 GB one (only the metadata index lives in memory).
- **Persistent indexing.** The first open creates a binary index (`.mboxshell.idx`) so subsequent opens take less than a second.
- **Full Gmail support.** Detects and displays `X-Gmail-Labels` as virtual folders in a sidebar panel, letting you filter by Inbox, Sent, Starred, custom labels, etc.
- **Correct encodings.** Decodes RFC 2047 encoded-words, supports UTF-8, ISO-8859-1, Windows-1252, KOI8-R, and any charset recognized by `encoding_rs`.
- **Conversation threading.** Groups messages into threads using the JWZ algorithm (the same one used by Netscape/Mozilla).
- **Advanced search.** Field-specific filtering (`from:`, `subject:`, `date:`, `body:`, `has:attachment`, `label:`, etc.), date ranges, size filters, AND/OR operators, and negation.
- **Flexible export.** Individual or bulk export to EML, CSV (Excel-compatible), plain text. Decoded attachment extraction.
- **Single binary.** No runtime, no dependencies. A ~5 MB executable that runs on Linux, macOS and Windows.
- **Full terminal UI.** Keyboard navigation (vi-style), three layout modes, interactive search bar, configurable shortcuts.
- **Bilingual.** Interface available in English and Spanish, auto-detected from system locale.

## Installation

### Pre-built binaries (recommended)

Download the latest release for your platform from the [Releases](https://github.com/dcarrero/mboxshell/releases) page:

| Platform | Binary |
|----------|--------|
| Linux x86_64 | `mboxshell-linux-x86_64` |
| Linux ARM64 | `mboxshell-linux-aarch64` |
| macOS Intel | `mboxshell-macos-x86_64` |
| macOS Apple Silicon | `mboxshell-macos-aarch64` |
| Windows x86_64 | `mboxshell-windows-x86_64.exe` |
| Windows ARM64 | `mboxshell-windows-aarch64.exe` |

After downloading, make it executable and move it to your PATH:

```bash
# Linux / macOS
chmod +x mboxshell-*
sudo mv mboxshell-* /usr/local/bin/mboxshell

# Or place it in a user-local directory
mv mboxshell-* ~/.local/bin/mboxshell
```

On Windows, move `mboxshell-windows-x86_64.exe` to a folder in your `PATH`, or run it directly.

### Build from source

Requirements: [Rust](https://www.rust-lang.org/tools/install) 1.85 or later.

```bash
# Clone and build
git clone https://github.com/dcarrero/mboxshell.git
cd mboxshell
cargo build --release

# The binary is at target/release/mboxshell
# Install it system-wide:
sudo cp target/release/mboxshell /usr/local/bin/

# Or for the current user only:
cp target/release/mboxshell ~/.local/bin/
```

#### Cross-compiling for other platforms

```bash
# Add the target you need
rustup target add aarch64-apple-darwin      # macOS Apple Silicon
rustup target add x86_64-unknown-linux-gnu  # Linux x86_64
rustup target add aarch64-unknown-linux-gnu # Linux ARM64

# Build for a specific target
cargo build --release --target aarch64-apple-darwin
```

### Install via Cargo

```bash
cargo install --git https://github.com/dcarrero/mboxshell.git
```

## Quick start

```bash
# Open an MBOX file in the terminal UI
mboxshell mail.mbox

# Index and show statistics
mboxshell index mail.mbox
mboxshell stats mail.mbox

# Search from the command line
mboxshell search mail.mbox "from:user@gmail.com date:2024"
mboxshell search mail.mbox "has:attachment subject:invoice" --json

# Export messages
mboxshell export mail.mbox --format eml --output ./emails/
mboxshell export mail.mbox --format csv --output summary.csv

# Extract attachments
mboxshell attachments mail.mbox --output ./attachments/

# Merge multiple MBOX files, removing duplicates
mboxshell merge file1.mbox file2.mbox -o merged.mbox --dedup

# Generate shell completions
mboxshell completions bash > /etc/bash_completion.d/mboxshell
mboxshell completions zsh > ~/.zfunc/_mboxshell
mboxshell completions fish > ~/.config/fish/completions/mboxshell.fish
```

## CLI commands

| Command | Description |
|---------|-------------|
| `mboxshell [FILE]` | Open a file in the TUI (default action) |
| `mboxshell open <path>` | Open a file or directory in the TUI |
| `mboxshell index <path> [-f/--force]` | Build or rebuild the binary index |
| `mboxshell stats <path> [--json]` | Show statistics about an MBOX file |
| `mboxshell search <path> <query> [--json]` | Search messages from the command line |
| `mboxshell export <path> -f <format> -o <output> [--query <q>]` | Export messages (formats: eml, csv, txt) |
| `mboxshell merge <files...> -o <output> [--dedup]` | Merge multiple MBOX files into one |
| `mboxshell attachments <path> -o <output>` | Extract all attachments |
| `mboxshell completions <shell>` | Generate shell completions (bash, zsh, fish, powershell) |
| `mboxshell manpage` | Generate a man page |

**Global flags:**

| Flag | Description |
|------|-------------|
| `-f`, `--force` | Force rebuild index even if one exists |
| `-v`, `--verbose` | Increase log verbosity (-v info, -vv debug, -vvv trace) |
| `--lang <en\|es>` | Force interface language (auto-detected by default) |

## Terminal UI

```
 mail.mbox | 142,857 / 142,857 messages | label: Inbox           [?] Help
+-Labels ----------++-Messages --------------------------------------------+
| All Messages (142|||   Date v           From                Subject       |
|-                 |||   2025-07-16 03:01 user@gmail.com      Re: Project.. |
| Inbox       (834)|||   2025-07-15 18:22 other@company.com   Budget pro.. |
| Sent       (1204)||| * 2025-07-15 10:00 maria@example.com   Meeting to.. |
| Starred      (47)|||   2025-07-14 22:15 noreply@service..   Your invoi.. |
| Important   (312)|||   ...                                                |
| Work        (205)||+-----------------------------------------------------+
| Personal    (189)||| Date:    Wed, 16 Jul 2025 03:01:03 +0000            |
|                  ||| From:    User <user@gmail.com>                       |
|                  ||| To:      Other <other@company.com>                   |
|                  ||| Subject: Re: Project Q3                              |
|                  ||| --------------------------------------------------- |
|                  ||| Hi,                                                  |
|                  |||                                                      |
|                  ||| Attached is the updated document...                  |
+------------------++------------------------------------------------------+
 j/k:Nav  /:Search  Enter:Open  s:Sort  e:Export  a:Attach  ?:Help  q:Quit
```

## Keyboard shortcuts

| Key | Action |
|-----|--------|
| `j` / `k` | Next / previous message |
| `g` / `G` | First / last message |
| `PgDn` / `PgUp` | Page down / up |
| `Enter` | Open message / switch to message view |
| `Tab` / `Shift-Tab` | Cycle panel focus |
| `Esc` | Back to list / close popup |
| `/` | Open search bar |
| `F` | Open search filter popup |
| `n` / `N` | Next / previous search result |
| `Space` | Mark / unmark message |
| `*` | Mark / unmark all |
| `s` | Cycle sort column (Date, From, Subject, Size) |
| `S` | Toggle sort direction |
| `e` | Export message (EML, TXT, CSV, Attachments) |
| `a` | Show attachments (j/k to navigate, Enter to save, A to save all) |
| `t` | Toggle threaded (conversation) view |
| `L` | Show / focus / hide labels sidebar |
| `h` | Toggle full headers |
| `r` | Toggle raw message source |
| `1` / `2` / `3` | Layout: list only / horizontal split / vertical split |
| `?` | Help |
| `q` | Quit |

## Search syntax

```
from:user@gmail.com              Search by sender
to:recipient@company.com         Search by recipient
subject:invoice                  Search in subject line
body:important text              Search in message body (full-text)
has:attachment                   Only messages with attachments
label:Inbox                      Filter by Gmail label
date:2024-01                     Messages from January 2024
date:2024-01-01..2024-06-30      Date range
before:2024-06-01                Before a date
after:2024-01-01                 After a date
size:>1mb                        Messages larger than 1 MB
-subject:spam                    Exclude messages with "spam" in subject
"exact phrase"                   Search for an exact phrase
from:john subject:budget         Implicit AND (both must match)
term1 OR term2                   Explicit OR
```

## Supported input formats

| Format | Extension | Description |
|--------|-----------|-------------|
| MBOX (mboxrd/mboxo) | `.mbox` | Standard format. Google Takeout, Thunderbird, Unix servers |
| EML | `.eml` | Individual RFC 5322 message |
| EML directory | (folder) | Folder containing multiple `.eml` files |

## Performance

Tested with real Google Takeout MBOX files:

| File size | Messages | Indexing | Re-open |
|-----------|----------|----------|---------|
| 500 MB | ~5,000 | ~3 s | < 1 s |
| 5 GB | ~50,000 | ~30 s | < 1 s |
| 50 GB | ~500,000 | ~5 min | < 1 s |

Message list navigation is instantaneous thanks to virtual scrolling (only visible rows are rendered).

## Configuration

The configuration file is located at `~/.config/mboxshell/config.toml`:

```toml
[general]
default_sort = "date"
sort_order = "desc"
date_format = "%Y-%m-%d %H:%M"
log_level = "warn"

[display]
theme = "dark"
layout = "horizontal"
show_sidebar = true
max_cached_messages = 50

[export]
default_format = "eml"
csv_separator = ","
```

## Architecture

```
src/
+-- main.rs              # CLI with clap
+-- lib.rs               # Module re-exports
+-- error.rs             # Error types with thiserror
+-- config.rs            # TOML configuration
+-- i18n/                # Internationalization (EN/ES)
+-- parser/
|   +-- mbox.rs          # Streaming parser (never loads the file into memory)
|   +-- eml.rs           # Individual EML file parser
|   +-- mime.rs          # MIME decoding, multipart, charsets
|   +-- header.rs        # RFC 5322 headers, RFC 2047 encoded-words
+-- index/
|   +-- builder.rs       # Binary index construction
|   +-- reader.rs        # Index queries
|   +-- format.rs        # Binary format with SHA-256 integrity check
+-- model/
|   +-- mail.rs          # MailEntry, MailBody
|   +-- attachment.rs    # Attachment metadata
|   +-- address.rs       # RFC 5322 address parsing
+-- store/
|   +-- reader.rs        # Offset-based reading with LRU cache
+-- search/
|   +-- query.rs         # Search query parser
|   +-- metadata.rs      # Fast index search (O(n), < 200ms for 1M messages)
|   +-- fulltext.rs      # Streaming full-text search
+-- export/
|   +-- eml.rs           # Export to .eml
|   +-- csv.rs           # Export summary to CSV (UTF-8 BOM)
|   +-- text.rs          # Export to plain text
|   +-- attachment.rs    # Attachment extraction
|   +-- mbox.rs          # MBOX merge with deduplication
+-- tui/
    +-- app.rs           # Global state (Elm Architecture)
    +-- event.rs         # Keyboard event handling
    +-- ui.rs            # Layout and render dispatch
    +-- threading.rs     # JWZ algorithm for conversation threads
    +-- theme.rs         # Color theme
    +-- widgets/         # Visual components
        +-- mail_list.rs       # List with virtual scrolling
        +-- mail_view.rs       # Message viewer with scroll
        +-- sidebar.rs         # Labels/folders panel
        +-- header_bar.rs      # Top bar
        +-- status_bar.rs      # Status bar
        +-- search_bar.rs      # Search bar
        +-- search_popup.rs    # Search filter popup
        +-- help_popup.rs      # Help popup
        +-- attachment_popup.rs # Attachment popup
        +-- export_popup.rs     # Export popup
```

## Key dependencies

| Crate | Purpose |
|-------|---------|
| `ratatui` + `crossterm` | Terminal UI |
| `mail-parser` | MIME/RFC 5322 parsing |
| `encoding_rs` | Charset decoding |
| `chrono` | Dates and time zones |
| `clap` | CLI argument parsing |
| `serde` + `bincode` | Index serialization |
| `sha2` | Index integrity verification |
| `lru` | Decoded message cache |
| `tracing` | Structured logging |

## Changelog

### v0.3.0
- Search filter popup (`F`): visual form to build queries without remembering syntax (from, to, subject, date range, size, attachment, label)
- Result counter in search bar: shows `(N / total)` while typing
- Search history: Up/Down arrow keys in the search bar navigate previous queries, with `[history]` indicator
- New help entries for `F` shortcut and search history hint

### v0.2.0
- Incremental search: message list filters as you type (metadata fields only; full-text runs on Enter)
- Dynamic message view title shows current mode: `[RAW]` or `[HEADERS]`
- Proportional PageDown/Up scroll in message view (adapts to actual viewport height)
- Improved thread indentation with vertical connectors (`│└`) and depth capped at 4 levels
- Added full CLI commands reference to documentation

### v0.1.2
- Active panel border highlighted in cyan for clear focus indicator
- Context-sensitive status bar: hints change depending on the focused panel
- Version number displayed at the bottom-right corner
- Help popup reorganized in multi-column layout (adapts to terminal width)
- Help popup now shows app name, version, license and author

### v0.1.0
- Initial release
- Streaming MBOX parser (handles 50 GB+ files without loading into memory)
- Persistent binary index for instant re-opens
- Full terminal UI with vi-style navigation and three layout modes
- Gmail labels support (X-Gmail-Labels) with sidebar filtering
- Advanced search: from:, to:, subject:, body:, date:, size:, has:attachment, label:
- Conversation threading (JWZ algorithm)
- Export to EML, TXT, CSV with attachment extraction
- Bilingual interface (English / Spanish)

## License

[MIT](LICENSE) - Copyright (c) 2026 David Carrero Fernandez-Baillo - [https://carrero.es](https://carrero.es)

Source Code: [https://github.com/dcarrero/mboxshell](https://github.com/dcarrero/mboxshell)
