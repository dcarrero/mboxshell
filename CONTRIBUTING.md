# Contributing to mboxshell

Thanks for taking the time to contribute. `mboxshell` is a small, focused terminal MBOX viewer written in Rust, and contributions of all sizes are welcome — bug reports, reproductions, documentation tweaks, performance work, new features, and translations.

This document explains how to get set up, what the project expects of code, and how to ship a change.

## Code of conduct

Be respectful, patient and assume good intent. This project follows the spirit of the [Contributor Covenant](https://www.contributor-covenant.org/). Harassment, personal attacks, and dismissive behaviour are not acceptable in issues, pull requests or any other project channel. Maintainers may remove comments, lock threads or block accounts when needed.

If you see something off, contact the maintainer at <https://carrero.es> or via a private GitHub message.

## Ways to contribute

- **Report a bug** — open an [issue](https://github.com/dcarrero/mboxshell/issues/new) with the smallest reproduction you can produce. Include `mboxshell --version`, OS and terminal, and a redacted MBOX excerpt when relevant. Screenshots of the TUI help.
- **Request a feature** — open an issue describing the use case before sending a PR. Small UX tweaks don't need a discussion first, but anything that changes the index format, the search syntax, or the CLI surface should be agreed before you write code.
- **Improve docs** — README, `CHANGELOG.md`, `CHANGELOG-ES.md`, code-level `///` doc comments. All welcome.
- **Translate** — the TUI and CLI strings live in `src/i18n/mod.rs` as a single `msg!(key, "English", "Español")` catalogue. Adding a third language means widening the macro and the runtime selector; please open an issue first so we can scope it together.
- **Send a patch** — see below.

## Security issues

**Do not open public issues for vulnerabilities.** See [SECURITY.md](SECURITY.md) for the private reporting channel.

## Development setup

### Prerequisites

- Rust **1.85** or newer (MSRV — see `rust-version` in `Cargo.toml`). Install via [rustup](https://rustup.rs).
- A POSIX-ish shell. Windows is supported as a target; building from Windows works too but the helper commands below assume bash/zsh.

```bash
git clone https://github.com/dcarrero/mboxshell.git
cd mboxshell

# Build and run the tests
cargo build
cargo test
```

### Day-to-day commands

```bash
cargo build                          # Debug build
cargo build --release                # Release build (use this for any benchmark)
cargo clippy -- -D warnings          # Lint — must pass with zero warnings
cargo fmt                            # Format
cargo test                           # Full unit + integration test suite
cargo test parser_tests              # Run a single test file
cargo bench                          # Criterion benchmarks (target/criterion/report/index.html)
cargo run -- index tests/fixtures/simple.mbox
cargo run -- tests/fixtures/simple.mbox     # Open in TUI
```

### Test fixtures

Small, self-contained MBOX/EML fixtures live in `tests/fixtures/`. **Never commit personal mail.** When you need to reproduce a parsing bug from a real message, redact addresses and bodies until only the structural shape remains.

If a fixture needs to be larger than a few KB (multipart trees, base64 attachments, charset edge cases), keep it minimal but real — generate it from a known-good MBOX rather than handcrafting bytes that don't represent real-world clients.

## Code style and rules

The project follows the rules in `CLAUDE.md`:

- Rust edition 2021, MSRV 1.85.
- `cargo clippy -- -D warnings` must pass — always.
- `cargo fmt` applied — always.
- Public functions need `///` doc comments.
- Errors: `thiserror` for library types, `anyhow` only in `src/main.rs`.
- **No `unwrap()` or `expect()` in production code** — tests are the only exception. Failures must propagate `MboxError` (or `anyhow::Result` in main).
- **No `unsafe`** — the codebase contains none.
- Logging is the `tracing` crate only. No `println!` outside intentional CLI output.
- All I/O operations must have timeouts or be cancelable.

A few additional norms worth knowing:

- **Streaming, always.** The parser must never load a whole MBOX into memory — files of 50 GB+ are an explicit target. New work that buffers an entire file or message body without justification will be sent back.
- **The index format is on disk** (`.mboxshell.idx`, magic + version + SHA256 of the first 4 KB). Bumping the on-disk schema means bumping `format::VERSION` and writing a migration or invalidation path. Don't change the layout silently.
- **The MBOX file is read-only.** mboxshell never writes back into the source MBOX. Exports and merges always go to a new file.

## Working with i18n

User-facing strings live in `src/i18n/mod.rs`. To add a new string:

1. Add a `msg!(short_name, "English text", "Texto en español");` line near the related keys.
2. Reference it from code as `i18n::short_name()`.
3. Do not hardcode English strings in the TUI or CLI — the language is auto-detected from the system locale or set via `--lang en|es`.

When you change a string, update both translations and check that the keys remain alphabetised within their section.

## Commit messages

The project uses short conventional-ish prefixes. Look at `git log --oneline` for examples. Typical shapes:

```
v0.3.3: fix Search Filters popup query building, surface F shortcut (#3, #4)
fix: tokenizer drops quoted spaces (#42)
docs: changelog CI line reflects actions v5 bump
ci: bump actions/checkout to v5
clippy: appease new lints from rustc 1.95 stable
```

A few rules:

- First line ≤ 70 chars when you can. Use the body for detail.
- Reference issues / PRs (`#N`) at the end of the line or in the body.
- **Do not add `Co-Authored-By: Claude`** or any other AI co-authorship trailer. Sign your own work.
- Don't skip hooks (`--no-verify`) or signing unless you have a real reason.

## Pull request workflow

1. Fork the repo and create a topic branch from `main` (e.g. `fix/search-quoted-phrases`).
2. Make the change, keep it focused. Bug fix + unrelated refactor in the same PR will usually be split.
3. **Add or update tests** — every bug fix needs a regression test that fails on `main` and passes on your branch. Search fixes go in `src/search/*::tests`; parser fixes in `tests/parser_tests.rs`; UI logic in `src/tui/app.rs::*_tests`.
4. Run the full local gate:
   ```bash
   cargo fmt
   cargo clippy -- -D warnings
   cargo test
   ```
5. Update `CHANGELOG.md` (English) and `CHANGELOG-ES.md` (Spanish, when you can) under an **Unreleased** section if no version is being cut, or under the version you're targeting if a release is in flight.
6. Open the PR against `main`. CI runs the same gate on Linux, macOS and Windows — please wait for green before pinging a review.
7. Address review feedback in new commits (don't force-push the branch unless asked). The maintainer will squash on merge if needed.

### What gets merged quickly

- Small bug fixes with a regression test.
- Documentation improvements.
- New tests that cover existing untested behaviour.
- Performance work backed by a benchmark in `benches/`.

### What gets pushed back

- PRs that introduce panics, `unwrap()` in non-test code, or any `unsafe`.
- Changes to the index format, search syntax or CLI flags without a prior issue / discussion.
- Drive-by formatting or refactor diffs unrelated to the stated goal.
- Anything that breaks `cargo clippy -- -D warnings` on the matrix.

## Releases

Releases are cut by the maintainer:

1. Bump `version` in `Cargo.toml`.
2. Add a section at the top of `CHANGELOG.md` and `CHANGELOG-ES.md`.
3. Commit, tag `vX.Y.Z`, push the tag — the release workflow builds binaries for Linux, macOS and Windows and publishes them on the [Releases page](https://github.com/dcarrero/mboxshell/releases).

If you want to propose a release (e.g. to ship a fix you contributed), say so in the PR description.

## License

By contributing you agree that your contributions will be licensed under the [MIT license](LICENSE) that covers the rest of the project.

## Thanks

Every reproducer, typo fix, and test counts. If something in this guide is unclear, open an issue — the docs are part of the project too.
