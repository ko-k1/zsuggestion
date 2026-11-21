# Project Status

Last reviewed: 2026-11-21

## Current State

zsuggestion 0.3.0 is an early-development Zsh completion system. It provides a
shared per-host daemon, SQLite-backed history, change-detected Zsh history
import, ranked command/option/subcommand/value/path/alias/history candidates,
native Zsh capture, and an inline ZLE popup with ghost text and previews.

The current slice includes:

- Shared auto-started daemon and versioned JSON protocol over an owner-private
  Unix socket.
- SQLite WAL history shared by all shells on a host, with change-detected Zsh
  history import and recency-first ranking. Current directory, successful
  exits, and usage act as deterministic tie-breakers.
- Cached `PATH` command inventory, shell builtins, semantic command icons, and
  authored descriptions for common tools.
- Lazy command-description enrichment from exact man pages and sandboxed
  `--help` output, with executable-fingerprinted persistent caching. Cache
  misses expire after one day; descriptions expire after 30 days or when the
  executable identity changes.
- Cached root subcommands and documented option values parsed from explicit
  help sections, including common Clap, Cobra, Click, and argparse layouts.
- Asynchronous native Zsh candidate capture for append-safe options,
  subcommands, paths, aliases, and other configured completion sources, with a
  native-completion fallback when zsuggestion has no high-confidence result.
- Prefix-only completion; Tab fills the highlighted candidate exactly.
- Cursor-anchored, scrolling ZLE popup with ghost text, descriptions, kind
  badges, highlighted matches, a position scrollbar, and configurable colors.
- Conservative segment acceptance, explicit fuzzy mode, previews, and
  debounced asynchronous requests that never block character input.

## Verified

- `cargo fmt --all -- --check` passes.
- `cargo clippy --all-targets --all-features --locked -- -D warnings` passes.
- `zsh -n -f src/shell/zsuggestion.zsh` passes.
- `cargo test --all-targets --all-features --locked` passes: 76 library tests,
  2 binary tests, and 1 ZLE integration test. The integration test skips
  itself when tmux is unavailable.
- `cargo test --doc --locked` passes (the crate has no doc tests).
- `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked` passes.
- `python3 init-agents.py --check` passes (15 Skills across the generated
  harness paths).
- `cargo package --locked --allow-dirty` passes and includes the documentation
  set. `cargo package --locked` itself requires a clean worktree.

## Partial or Missing

- Nested subcommand metadata and typed positional arguments are not implemented.
- Optional project-aware providers are not implemented.
- The crate is not yet published on crates.io; installation is from a checkout.
- Live configuration reload is planned; the daemon reads configuration at
  startup.
- Native Zsh capture accepts only append-safe candidates. Quoting, in-word
  replacement, and other native edit semantics remain delegated to Zsh.
- Image and PDF candidates show bounded metadata rather than graphics.
- Direct executable `--help` probing runs through a sandbox only on macOS;
  other platforms use man pages only.
- `cargo package --locked` requires a clean worktree, so the two untracked
  work-in-progress files (`src/commands/known.rs`, `src/engine/history.rs`)
  block packaging until they are committed or removed.
- The CI MSRV job currently tests Rust 1.120.0 while `Cargo.toml` declares
  Rust 1.89; the declared minimum is not exercised by that job.

## Unverified

- Behavior across terminal emulators and non-UTF-8 locales. The integration has
  an ASCII fallback, but the migration environment did not exercise it.
- Performance with very large history databases or many concurrent shells.
- The ZLE integration test in this environment, because tmux is unavailable and
  the test skipped.
- The declared Rust 1.89 minimum on a matching toolchain; local checks used
  Rust 1.100.0.

This file describes present reality, not planned work. Keep plans in
[ROADMAP.md](ROADMAP.md), and avoid treating a passing test as evidence for
behavior the test does not exercise.
