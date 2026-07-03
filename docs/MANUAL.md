# mboxShell — User Manual

> Complete guide to every feature of `mboxShell`, the fast terminal MBOX viewer.
> **Applies to mboxShell v0.6.0.**
> Spanish version: [MANUAL-ES.md](MANUAL-ES.md) · Short overview: [../README.md](../README.md) · Changes: [../CHANGELOG.md](../CHANGELOG.md)

`mboxShell` opens, searches and exports `.mbox` files of any size (50 GB+) from the terminal, without ever loading the whole file into memory and **without ever modifying the source file** (it is strictly read-only).

---

## Table of contents

1. [Core concepts](#1-core-concepts)
2. [Installation](#2-installation)
3. [Quick start](#3-quick-start)
4. [Command-line reference](#4-command-line-reference)
5. [The terminal UI (TUI)](#5-the-terminal-ui-tui)
6. [Keyboard shortcuts](#6-keyboard-shortcuts)
7. [Search](#7-search)
8. [Export & extraction](#8-export--extraction)
9. [Configuration file](#9-configuration-file)
10. [Environment variables](#10-environment-variables)
11. [Language / internationalization](#11-language--internationalization)
12. [Shell completions & man page](#12-shell-completions--man-page)
13. [Performance & limits](#13-performance--limits)
14. [Troubleshooting & FAQ](#14-troubleshooting--faq)

---

## 1. Core concepts

| Concept | What it means |
|---------|---------------|
| **Read-only** | mboxShell never writes to your `.mbox`. Exports and merges always go to new files you specify. |
| **Streaming I/O** | The file is read in chunks (128 KB buffer by default). A 100 GB mailbox uses roughly the same RAM as a 1 GB one. |
| **Binary index** | On first open, an index file `<name>.mboxshell.idx` is created next to the MBOX. It holds compact metadata (sender, subject, date, offsets) so subsequent opens take under a second. |
| **Index validation** | The index is tied to the source via file size, modification time and a SHA-256 of the file's first bytes. If the MBOX changes, the index is rebuilt automatically. |
| **On-demand bodies** | Message bodies are decoded only when you open a message, then kept in a small LRU cache (50 messages by default). |
| **Gmail labels** | `X-Gmail-Labels` headers (from Google Takeout) are surfaced as virtual folders in a sidebar. |

### Supported input formats

| Format | Path | Notes |
|--------|------|-------|
| MBOX (mboxrd / mboxo) | `file.mbox` | Google Takeout, Thunderbird, Unix servers |
| EML | `message.eml` | A single RFC 5322 message |
| EML directory | `folder/` | A folder containing several `.eml` files |

---

## 2. Installation

### Pre-built binaries (recommended)

Download the binary for your platform from the [Releases](https://github.com/dcarrero/mboxshell/releases) page:

| Platform | Binary |
|----------|--------|
| Linux x86_64 | `mboxshell-linux-x86_64` |
| Linux ARM64 | `mboxshell-linux-aarch64` |
| Linux RISC-V 64 | `mboxshell-linux-riscv64` |
| FreeBSD x86_64 | `mboxshell-freebsd-x86_64` |
| macOS Intel | `mboxshell-macos-x86_64` |
| macOS Apple Silicon | `mboxshell-macos-aarch64` |
| Windows x86_64 | `mboxshell-windows-x86_64.exe` |
| Windows ARM64 | `mboxshell-windows-arm64.exe` |

```bash
# Linux / macOS
chmod +x mboxshell-*
sudo mv mboxshell-* /usr/local/bin/mboxshell      # system-wide
# or:
mv mboxshell-* ~/.local/bin/mboxshell              # current user
```

On Windows, move the `.exe` into a folder on your `PATH`, or run it directly.

### Build from source

Requires [Rust](https://www.rust-lang.org/tools/install) 1.85 or later.

```bash
git clone https://github.com/dcarrero/mboxshell.git
cd mboxshell
cargo build --release
sudo cp target/release/mboxshell /usr/local/bin/
```

### Install via Cargo

```bash
cargo install --git https://github.com/dcarrero/mboxshell.git
```

> **macOS GUI alternative:** if you prefer a native graphical app, see [mboxViewer](https://mboxviewer.net) — same parsing engine, desktop interface.

---

## 3. Quick start

```bash
# Open a mailbox in the interactive viewer (default action)
mboxshell mail.mbox

# Build the index and print statistics
mboxshell index mail.mbox
mboxshell stats mail.mbox

# Search from the command line
mboxshell search mail.mbox "from:user@gmail.com date:2024"

# Export to individual .eml files
mboxshell export mail.mbox --format eml --output ./emails/

# Extract every attachment
mboxshell attachments mail.mbox --output ./attachments/

# Merge several mailboxes into one, dropping duplicates
mboxshell merge a.mbox b.mbox -o merged.mbox --dedup
```

---

## 4. Command-line reference

General form:

```
mboxshell [GLOBAL FLAGS] [COMMAND] [ARGS]
mboxshell [GLOBAL FLAGS] <FILE>        # no command = open <FILE> in the TUI
```

### Global flags

| Flag | Description |
|------|-------------|
| `-f`, `--force` | Force a full index rebuild even if a valid index exists |
| `-v`, `-vv`, `-vvv` | Increase log verbosity (`info`, `debug`, `trace`) |
| `--lang <en\|es>` | Force the interface language (auto-detected from the locale by default) |
| `-h`, `--help` | Show help |
| `-V`, `--version` | Show version |

### Commands

| Command | Purpose |
|---------|---------|
| `mboxshell <FILE>` | Open a file/directory in the TUI (default when no subcommand is given) |
| `open <path>` | Open a file or directory in the TUI |
| `index <path>` | Build or rebuild the binary index (use `--force` to rebuild) |
| `stats <path> [--json]` | Print statistics (message count, date range, top senders, …) |
| `search <path> <query> [--json]` | Search and print matching messages |
| `export <path> -o <out> [options]` | Export messages (see below) |
| `merge <inputs...> -o <out> [--dedup]` | Merge several MBOX files into one |
| `attachments <path> -o <out>` | Extract all attachments into a directory |
| `completions <shell>` | Print shell completion script (`bash`, `zsh`, `fish`, `powershell`, `elvish`) |
| `manpage` | Print a man page to stdout |

#### `export` options

| Option | Description |
|--------|-------------|
| `-f`, `--format <fmt>` | `eml` (default), `csv`, `txt` (or `text`), `html` |
| `-o`, `--output <path>` | Output directory (per-message formats) or file (csv) — **required** |
| `--query <q>` | Only export messages matching this [search query](#7-search) |
| `--qp` | Re-encode 8-bit text as quoted-printable so the `.eml` is pure 7-bit ASCII (helps strict tools like `eml-extractor`). **EML only.** |
| `--raw-html` | Keep the original HTML body **unsanitized** (scripts, `on*`, iframes preserved). For local archival only — never serve these files. **HTML only.** |

#### `stats` output

`stats` reports: file path and size, message count, date range (oldest/newest), index size, indexing time, count and percentage of messages with attachments, the number of duplicate `Message-ID`s (with the count of unique IDs), and the top 10 senders. Add `--json` for a machine-readable object.

#### Examples

```bash
mboxshell stats mail.mbox --json
mboxshell search mail.mbox "has:attachment subject:invoice" --json
mboxshell export mail.mbox -f csv -o summary.csv
mboxshell export mail.mbox -f eml -o ./out/ --query "from:boss after:2024-01-01" --qp
mboxshell export mail.mbox -f html -o ./html/ --raw-html
mboxshell completions zsh > ~/.zfunc/_mboxshell
```

---

## 5. The terminal UI (TUI)

Launching `mboxshell <file>` (or `open`) starts the interactive viewer.

### Panels

- **Header bar** (top): file name and global context.
- **Message list**: virtual-scrolled table of messages (Date, From, Subject, Size). Only visible rows are rendered, so navigation is instant even on 500 000-message mailboxes.
- **Message view**: the decoded message. A scroll-position indicator appears in the bottom-right of its border — `[ All ]` when the whole body fits, `[ ↓ Top ]` at the start, `[ ↕ NN% ]` in the middle, `[ ↑ Bot ]` at the end.
- **Labels sidebar** (optional): Gmail labels / folders; select one to filter the list.
- **Status bar / search bar** (bottom): hints, search progress, or the active query.

### Layout modes

Switch any time with the number keys:

| Key | Layout |
|-----|--------|
| `1` | List only (full-screen list; `Enter` shows the message full-screen) |
| `2` | Horizontal split (list on top, message below) — default |
| `3` | Vertical split (list left, message right) |

### Message view modes

- **Default**: compact headers (Date, From, To, Cc, Subject) + decoded body, with URLs highlighted.
- `h` — toggle **full headers** (every raw header line).
- `r` — toggle **raw source** (the original message bytes).
- `H` — open the **HTML body in an external viewer** (see [`MBOXSHELL_HTML_VIEWER`](#10-environment-variables)).

### Reading long messages

Scroll the body **without leaving the list** using `Shift-↑` / `Shift-↓` (and `Shift-PageUp` / `Shift-PageDown`). The plain arrow keys keep navigating the list. The position indicator in the message border tells you at a glance whether there is more to read.

### Searching within a message

When a message is open and the **message view is focused** (press `Enter` to switch to it), press `/` to search inside the body, *less/vim* style:

- Type your term — every match is **highlighted live** as you type, and the view jumps to the first one.
- `Enter` confirms the search: the prompt closes but the highlights stay and the matches remain navigable.
- `n` / `N` move to the **next / previous** match, auto-scrolling to bring it into view; the focused match is emphasised.
- A `[ current/total ]` counter appears in the message border, next to the scroll indicator (e.g. `[ 3/12 ]`).
- `Esc` clears the matches; a second `Esc` returns to the list.

Matching is **case-insensitive** and Unicode-aware. This is separate from the global search: `/` from the list or sidebar still searches *across messages*; `/` with the message view focused searches *within the open body*.

### Labels sidebar

Press `l` to show / focus / hide the sidebar (only populated when the mailbox has `X-Gmail-Labels`). Selecting a label scopes the list to that label; subsequent searches stay within it.

### Threading

Press `t` to toggle the **conversation (threaded) view**, which groups messages into threads using the JWZ algorithm (the same one Netscape/Mozilla used). Press `t` again to return to the flat list.

### Marking messages

- `Space` — mark / unmark the current message.
- `*` — mark / unmark all visible messages.

Marks let you act on a selection (e.g. export).

### Attachments

Press `a` to open the attachment popup for the current message:

- `j` / `k` — move between attachments
- `Enter` — save the highlighted attachment
- `A` — save all attachments
- `Esc` / `a` — close

### Sorting

- `s` — cycle the sort column: Date → From → Subject → Size.
- `S` — toggle ascending / descending.

---

## 6. Keyboard shortcuts

### Global / message list

| Key | Action |
|-----|--------|
| `j` / `k` (or `↓` / `↑`) | Next / previous message |
| `g` / `G` (or `Home` / `End`) | First / last message |
| `PgDn` / `PgUp` | Page down / up |
| `Enter` | Open message / switch to message view |
| `Shift-↑` / `Shift-↓` | Scroll the selected message body (keeps list focus) |
| `Shift-PageUp` / `Shift-PageDown` | Page-scroll the message body |
| `Tab` / `Shift-Tab` | Cycle panel focus |
| `/` | Open the search bar |
| `f` | Open the search filter popup (`F` is a hidden alias) |
| `n` / `N` | Next / previous search result |
| `Space` | Mark / unmark message |
| `*` | Mark / unmark all |
| `s` / `S` | Cycle sort column / toggle sort direction |
| `e` | Export the current message (EML, TXT, CSV, attachments) |
| `a` | Show attachments |
| `t` | Toggle threaded (conversation) view |
| `l` | Show / focus / hide the labels sidebar (`L` alias) |
| `h` | Toggle full headers |
| `H` | Open the HTML body in an external viewer |
| `r` | Toggle raw message source |
| `1` / `2` / `3` | Layout: list only / horizontal split / vertical split |
| `?` | Help |
| `Esc` | Back to list / close popup |
| `q` or `Ctrl-C` | Quit |

### Message view — in-body search (press `/` with the message view focused)

The search prompt opens at the **top of the message panel**, next to the body.

| Key | Action |
|-----|--------|
| `/` | Open the in-body search prompt |
| *(type)* | Refine the query; matches highlight live and the view jumps to the first |
| `Enter` | Confirm — close the prompt, keep highlights and `n`/`N` navigation |
| `n` / `N` | Jump to the next / previous match (auto-scrolls to it) |
| `Esc` | Clear the matches; press again to return to the list |

### Search bar (after pressing `/`)

| Key | Action |
|-----|--------|
| *(type)* | Edit the query; the list filters live for metadata queries |
| `Enter` | Run the search (body/full-text searches run on a background thread) |
| `↑` / `↓` | Browse search history |
| `Esc` | Cancel and restore the previous view |

### Search filter popup (after pressing `f`)

| Key | Action |
|-----|--------|
| `Tab` / `↑` / `↓` | Move between fields (`Shift-Tab` moves back) |
| `PgUp` / `PgDn` (or `Home` / `End`) | Jump to the first / last field |
| *(type)* | Fill the focused field (Text, From, To, Subject, dates…) |
| `Space` | Toggle the focused checkbox (`has:attachment`, *Search within previous results*) |
| `←` / `→` (or `j` / `k`) | Change the Size / Label selector value |
| `Enter` | Build the query and run it |
| `Esc` | Close the popup |

### Attachment popup (after pressing `a`)

| Key | Action |
|-----|--------|
| `j` / `k` | Move between attachments |
| `Enter` | Save the highlighted attachment |
| `A` | Save all |
| `Esc` / `a` | Close |

---

## 7. Search

mboxShell has one query language used by both the CLI `search` command and the in-app search bar / filter popup.

### Two engines

- **Metadata search** — matches subject, from, to, cc, labels, dates, size, attachments. Runs against the in-memory index, so it is instant (under ~200 ms even for a million messages) and filters the list *as you type*.
- **Full-text search** — triggered by `body:` or a bare free-text term. It streams the message bodies from disk, so it runs **on a background thread**: the UI stays responsive, shows live progress (`Searching message bodies N/M`), and can be cancelled with `Esc`.

### Query syntax

| Operator | Meaning | Example |
|----------|---------|---------|
| *(bare word)* | Search subject + from + to; a bare word also triggers a body scan on `Enter` | `invoice` |
| `from:` | Sender | `from:user@gmail.com` |
| `to:` | Recipient | `to:team@company.com` |
| `cc:` | Carbon copy | `cc:boss@company.com` |
| `subject:` | Subject line | `subject:budget` |
| `body:` | Full-text body search | `body:contract signed` |
| `label:` | Gmail label | `label:Inbox` |
| `filename:` | Attachment file name | `filename:report.pdf` |
| `id:` | Message-ID | `id:<abc@domain>` |
| `has:attachment` | Only messages with attachments | `has:attachment` |
| `has:no-attachment` | Only messages without attachments | `has:no-attachment` |
| `date:` | Exact day / month / year, or a range | `date:2024-01-15`, `date:2024-01`, `date:2024`, `date:2024-01-01..2024-06-30` |
| `before:` / `after:` | Open-ended date bounds | `before:2024-06-01`, `after:2024-01-01` |
| `size:` | Size comparison | `size:>1mb`, `size:<100kb` |
| `"…"` | Quoted exact phrase | `subject:"monthly report"` |
| *(space)* | Implicit **AND** — all terms must match | `from:john subject:budget` |
| `OR` | Explicit **OR** — any term matches | `from:alice OR from:bob` |
| `-` | **NOT** — exclude | `-subject:spam` |

### Multi-word free text

A multi-word value in the popup's **Text** field (or a bare multi-word query) matches messages containing **all** the words (AND), searched across subject/from/to **and** the body — not the exact contiguous phrase. Use quotes (`"…"`) when you need the literal phrase.

### Search within previous results

In the filter popup, the **Search within previous results** checkbox confines the next query to the messages currently visible. It is a **persistent scoping mode**: once enabled it stays on across popup reopens, so you can iteratively refine (e.g. narrow by Subject, then by a body word, then by sender). The mode is dropped automatically only when the scope itself resets — when you change the sidebar label filter, or leave the search with `Esc`.

### CLI search

```bash
mboxshell search mail.mbox "from:user@gmail.com date:2024"
mboxshell search mail.mbox "has:attachment subject:invoice" --json
```

`--json` prints structured results for scripting.

---

## 8. Export & extraction

### Export formats

| Format | `--format` | Output | Notes |
|--------|-----------|--------|-------|
| EML | `eml` (default) | one `.eml` per message in the output directory | Add `--qp` for pure 7-bit ASCII bodies |
| CSV | `csv` | a single `.csv` summary file | UTF-8 with BOM (Excel-friendly); separator configurable |
| Plain text | `txt` / `text` | one `.txt` per message | Decoded text body |
| HTML | `html` | one standalone `.html` per message | Body sanitized by default; `--raw-html` keeps it untouched (local archival only) |

Combine with `--query` to export only matching messages:

```bash
mboxshell export mail.mbox -f eml -o ./out/ --query "label:Important after:2023-01-01"
```

In the TUI, press `e` on a message to open the export popup and choose a format interactively.

### Merging mailboxes

```bash
mboxshell merge inbox.mbox archive.mbox -o all.mbox --dedup
```

`merge` concatenates several MBOX files into one. `--dedup` (on by default) removes duplicate messages (by Message-ID / content), so merging overlapping Takeout exports is safe.

### Extracting attachments

```bash
mboxshell attachments mail.mbox -o ./attachments/
```

Decodes and writes every attachment across the whole mailbox to the output directory. For a single message, use the `a` popup in the TUI instead.

---

## 9. Configuration file

Configuration is optional — mboxShell works out of the box. When present, the file is read from:

1. `$MBOXSHELL_CONFIG` (if set), otherwise
2. `~/.config/mboxshell/config.toml` (Linux/macOS) · `%APPDATA%\mboxshell\config.toml` (Windows)

An invalid or missing file falls back to defaults silently. Full file with the **real default values**:

```toml
[general]
default_sort = "date"          # date | from | subject | size
sort_order   = "desc"          # desc | asc
date_format  = "%Y-%m-%d %H:%M"
# cache_dir  = "/custom/path"  # default: OS cache dir + /mboxshell
log_level    = "warn"          # error | warn | info | debug | trace

[display]
theme               = "dark"        # dark | light
layout              = "horizontal"  # horizontal | vertical | list-only
show_sidebar        = false         # show the labels sidebar on start
max_cached_messages = 50
message_text_width  = 0             # 0 = use full panel width

[columns]
date_width = 17
from_width = 20
size_width = 8

[export]
default_format = "eml"          # eml | csv | txt | html
# default_output_dir = "./out"
csv_separator  = ","

[performance]
read_buffer_size = 131072       # 128 KB streaming buffer
max_message_size = 268435456    # 256 MB cap per message
lru_cache_size   = 50           # decoded messages kept in memory
```

Related paths:

- **Index**: `<mailbox>.mboxshell.idx`, next to the source file.
- **Cache directory**: `cache_dir`, or the OS cache dir + `/mboxshell`.
- **Log file**: `<cache directory>/mboxshell.log`.

---

## 10. Environment variables

| Variable | Effect |
|----------|--------|
| `MBOXSHELL_CONFIG` | Absolute path to a config file, overriding the standard location |
| `MBOXSHELL_HTML_VIEWER` | External command used by `H` to render HTML bodies. Defaults to `w3m`. Works with `chawan`, `lynx -dump`, `pandoc`, etc. The TUI suspends itself while the viewer runs and restores cleanly on exit. |
| `MBOXSHELL_LANG` | Force the interface language (`en` / `es`). Takes precedence over `LC_MESSAGES` and `LANG`. |

```bash
MBOXSHELL_HTML_VIEWER="lynx -dump" mboxshell mail.mbox
MBOXSHELL_LANG=es mboxshell mail.mbox
```

---

## 11. Language / internationalization

The interface and CLI output are available in **English** and **Spanish**. Language is resolved in this order:

1. `--lang en|es` flag
2. `MBOXSHELL_LANG`
3. `LC_MESSAGES`
4. `LANG`
5. fall back to English

```bash
mboxshell --lang es mail.mbox
```

---

## 12. Shell completions & man page

```bash
# Bash
mboxshell completions bash | sudo tee /etc/bash_completion.d/mboxshell

# Zsh (a directory on your $fpath)
mboxshell completions zsh > ~/.zfunc/_mboxshell

# Fish
mboxshell completions fish > ~/.config/fish/completions/mboxshell.fish

# PowerShell / Elvish are also supported
mboxshell completions powershell > mboxshell.ps1

# Man page
mboxshell manpage > mboxshell.1
```

---

## 13. Performance & limits

Measured on real Google Takeout exports:

| File size | Messages | First-time indexing | Re-open |
|-----------|----------|---------------------|---------|
| 500 MB | ~5 000 | ~3 s | < 1 s |
| 5 GB | ~50 000 | ~30 s | < 1 s |
| 50 GB | ~500 000 | ~5 min | < 1 s |

- RAM stays roughly flat regardless of file size — only the metadata index lives in memory.
- A single message larger than `max_message_size` (256 MB by default) is skipped to protect memory.
- List navigation is O(1) thanks to virtual scrolling.

---

## 14. Troubleshooting & FAQ

**Does mboxShell modify my mailbox?**
No. It is strictly read-only. Every export/merge writes to a new path you choose.

**The first open is slow.**
That is the one-time indexing pass. Subsequent opens read the `.mboxshell.idx` and are near-instant. Force a rebuild with `mboxshell index <file> --force` if the index ever looks stale (it is normally rebuilt automatically when the source changes).

**A body search seems to hang.**
Full-text (`body:` / bare words) scans the file on a background thread. Watch the `Searching message bodies N/M` progress in the status bar, and press `Esc` to cancel.

**`H` does nothing / errors.**
It needs an external text-mode HTML viewer. Install `w3m` (default) or set `MBOXSHELL_HTML_VIEWER` to one you have (`chawan`, `lynx -dump`, `pandoc`, …).

**Accents look wrong.**
mboxShell decodes RFC 2047 encoded-words and most charsets via `encoding_rs`. If something still looks off, view the raw source with `r` to confirm the original encoding.

**Where are logs?**
In `<cache directory>/mboxshell.log`. Increase detail with `-v` / `-vv` / `-vvv` and `log_level` in the config.

---

*See also: [README.md](../README.md) · [CHANGELOG.md](../CHANGELOG.md) · [MANUAL-ES.md](MANUAL-ES.md)*

---

## License

MIT - Copyright (c) 2026 David Carrero Fernandez-Baillo - [https://carrero.es](https://carrero.es)

Source Code: [https://github.com/dcarrero/mboxshell](https://github.com/dcarrero/mboxshell)
