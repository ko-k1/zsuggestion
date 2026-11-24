# Project Status

Last reviewed: 2026-11-24

## Current state

zsuggestion 0.3.0 is early-development Zsh completion software. It provides a
shared per-host daemon, SQLite-backed history, change-detected Zsh history
import, ranked command/option/subcommand/value/path/alias/history candidates,
native Zsh capture, and an inline ZLE popup with ghost text and previews.

The current slice includes:

- Shared auto-started daemon with a versioned JSON protocol over an
  owner-private Unix socket.
- SQLite WAL history shared by all shells on a host, with change-detected Zsh
  history import and recency-first ranking.
- Cached `PATH` command inventory, shell builtins, semantic command icons, and
  authored descriptions for common tools.
- Lazy command-description enrichment from exact man pages and sandboxed
  `--help` output, with persistent caching keyed by executable identity.
- Cached root subcommands and documented option values parsed from help
  sections.
- Asynchronous native Zsh candidate capture for append-safe options,
  subcommands, paths, aliases, and other completion sources, falling back to
  native Zsh completion when zsuggestion has no high-confidence result.
- Prefix-only completion; Tab fills the highlighted candidate exactly.
- Cursor-anchored, scrolling ZLE popup with ghost text, descriptions, kind
  badges, highlighted matches, a position scrollbar, and configurable colors.
- `Ctrl-R` history search in the completion menu, ranked by `fzf` when
  installed, with a built-in word matcher otherwise.
- Conservative segment acceptance, explicit fuzzy mode, previews, and
  debounced asynchronous requests that never block character input.

## Verified

- `cargo fmt --all -- --check`, `cargo clippy`, and `cargo test` pass
  (97 library tests and 12 ZLE integration tests; the integration binary exits
  early when tmux or zsh is unavailable).
- `zsh -n` passes on the shell integration.
- `cargo package` passes and includes the documentation set.

## Partial or missing

- Nested subcommand metadata and typed positional arguments are not implemented.
- Optional project-aware providers are not implemented.
- Not yet published on crates.io; install from a checkout.
- Live configuration reload is planned; the daemon reads configuration at
  startup.
- Native Zsh capture accepts only append-safe candidates. Quoting, in-word
  replacement, and other native edit semantics remain delegated to Zsh.
- Image and PDF candidates show bounded metadata rather than graphics.
- Direct executable `--help` probing runs through a sandbox only on macOS;
  other platforms use man pages only.
- The declared Rust 1.89 minimum is not exercised by the CI MSRV job (which
  tests Rust 1.120.0).

## Unverified

- Behavior across terminal emulators and non-UTF-8 locales (an ASCII fallback
  exists but was not exercised here).
- Performance with very large history databases or many concurrent shells.
- The ZLE integration test (skipped here because tmux is unavailable).
- The declared Rust 1.89 minimum on a matching toolchain.

This file describes present reality, not planned work. Keep plans in
[ROADMAP.md](ROADMAP.md), and avoid treating a passing test as evidence for
behavior the test does not exercise.
