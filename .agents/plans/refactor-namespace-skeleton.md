# Plan: namespace & skeleton refactor (polished)

## Status
Ready for review — no files touched yet.

## Branch
`refactor/namespace-skeleton` created from `ecf0975`, working tree clean.

## Baseline (verified green)
- `cargo fmt --all -- --check`: clean
- `cargo clippy --all-targets --all-features --locked -- -D warnings`: clean
- Tests: 79 lib + 2 bin pass; doc tests pass
- CI also runs `zsh -n -f src/shell/zsuggestion.zsh` and `cargo package --locked`

## Goal
Pure namespace/skeleton restructure: move related files into coherent module directories and rename modules to match their domain responsibility. **Zero logic changes.** Every public path (`crate::X`) that changes is accounted for below.

---

## Architectural review (verified facts)

| Layer | Current files | Problem | New home |
|---|---|---|---|
| CLI entry | `src/main.rs` | thin clap dispatcher | unchanged |
| Crate root | `src/lib.rs` | flat module map | rewritten |
| Time | `src/clock.rs` | single responsibility | unchanged (`src/clock.rs`) |
| Settings + paths | `src/config.rs` | strict TOML + XDG paths | `src/config/mod.rs` |
| Serialization | `src/output.rs` | JSON / NUL-separated formats | `src/output/mod.rs` |
| Previews | `src/preview.rs` | bounded file + command previews | `src/preview/mod.rs` |
| Frontend renderer | `src/shell.rs` + template | `include_str!` beside template | `src/shell/mod.rs` + `src/shell/zsuggestion.zsh` |
| History persistence | `src/store.rs` | really a SQLite history store (`command_events`, `history_imports`, schema v1) | `src/persistence/mod.rs` |
| Command inventory | `src/commands/` | catalog + lazy enrichment mixed | `src/catalog/` |
| Completion providers | `src/engine/` | tiered providers behind `complete()` | `src/providers/` |
| IPC transport | `client.rs` + `daemon.rs` + `protocol.rs` flat | one socket boundary split across 3 top-level files | `src/transport/{client,daemon,protocol}.rs` |

### Why these names
- `store` → `persistence`: the module owns SQLite WAL history; `store` is domain-vague and collides conceptually with in-memory caches.
- `engine` → `providers`: `complete()` orchestrates tiered candidate providers; `engine` implies a monolith.
- `commands` → `catalog`: the module is an installed-command inventory with optional enrichment; `catalog` is precise.
- `client`/`daemon`/`protocol` → `transport`: they form one versioned JSON-over-Unix-socket IPC layer.
- `config`/`output`/`preview`/`shell`/`clock`: single-responsibility, kept as modules but converted to `mod.rs` directories for structural consistency with the rest.

---

## Exact file moves

All moves via `git mv` to preserve history:

```
src/config.rs                      → src/config/mod.rs
src/output.rs                      → src/output/mod.rs
src/preview.rs                     → src/preview/mod.rs
src/shell.rs                       → src/shell/mod.rs
                                   (template stays at src/shell/zsuggestion.zsh;
                                    include_str! path changes below)
src/client.rs                      → src/transport/client.rs
src/daemon.rs                      → src/transport/daemon.rs
src/protocol.rs                    → src/transport/protocol.rs
src/commands/mod.rs                → src/catalog/mod.rs
src/commands/known.rs              → src/catalog/known.rs
src/commands/parse.rs              → src/catalog/parse.rs
src/commands/probe.rs              → src/catalog/probe.rs
src/commands/cache.rs              → src/catalog/cache.rs
src/engine/mod.rs                  → src/providers/mod.rs
src/engine/alias.rs                → src/providers/alias.rs
src/engine/context.rs              → src/providers/context.rs
src/engine/filesystem.rs           → src/providers/filesystem.rs
src/engine/fuzzy.rs                → src/providers/fuzzy.rs
src/engine/history.rs              → src/providers/history.rs
src/store.rs                       → src/persistence/mod.rs
```

Files NOT moved (responsibility unchanged): `src/main.rs`, `src/lib.rs`, `src/clock.rs`, `tests/zsh_ui.rs`, `src/shell/zsuggestion.zsh`.

---

## New file: `src/transport/mod.rs`

```rust
//! Versioned JSON protocol and Unix-socket transport (client + daemon).

pub mod client;
pub mod daemon;
pub mod protocol;
```

This replaces the three flat top-level modules `client`, `daemon`, `protocol` with one `transport` namespace.

---

## `src/lib.rs` module map (rewritten)

```rust
pub mod catalog;
pub mod clock;
pub mod config;
pub mod output;
pub mod persistence;
pub mod preview;
pub mod providers;
pub mod shell;
pub mod transport;
```

Removed: `client`, `commands`, `daemon`, `engine`, `protocol`, `store`.

Added: `catalog`, `persistence`, `providers`, `transport`.

Ordering is alphabetical for scannability.

---

## Exact import changes (every `use crate::` / `use zsuggestion::` affected)

### `src/main.rs`
- `use zsuggestion::protocol::{Alias, Request, Response};` → `use zsuggestion::transport::protocol::{Alias, Request, Response};`
- `use zsuggestion::{client, clock, daemon, preview, shell};` → `use zsuggestion::{clock, preview}; use zsuggestion::transport::{client, daemon}; use zsuggestion::shell;`
- (`config`, `output` unchanged.)

### `src/transport/client.rs` (was `src/client.rs`)
- `use crate::protocol::{PROTOCOL_VERSION, Request, RequestEnvelope, Response};` → `use crate::transport::protocol::{PROTOCOL_VERSION, Request, RequestEnvelope, Response};`
- (`use crate::config::Paths;` unchanged.)

### `src/transport/daemon.rs` (was `src/daemon.rs`)
- `use crate::commands::CommandCatalog;` → `use crate::catalog::CommandCatalog;`
- `use crate::engine;` → `use crate::providers;`
- `use crate::protocol::{PROTOCOL_VERSION, Request, RequestEnvelope, Response};` → `use crate::transport::protocol::{PROTOCOL_VERSION, Request, RequestEnvelope, Response};`
- `use crate::store::Store;` → `use crate::persistence::Store;`
- (`crate::VERSION`, `crate::clock::now_ms`, `crate::config::{Paths, Settings}` unchanged.)

### `src/shell/mod.rs` (was `src/shell.rs`)
- `include_str!("shell/zsuggestion.zsh");` → `include_str!("zsuggestion.zsh");` (path relative to `shell/mod.rs`)
- (`use crate::config::{Settings, completion_key_sequence};` unchanged.)

### `src/output/mod.rs` (was `src/output.rs`)
- `use crate::protocol::CompletionResponse;` → `use crate::transport::protocol::CompletionResponse;`

### `src/providers/mod.rs` (was `src/engine/mod.rs`)
- `use crate::commands::CommandCatalog;` → `use crate::catalog::CommandCatalog;`
- `use crate::protocol::{Alias, Candidate, CandidateKind, CandidateSource, CompletionResponse};` → `use crate::transport::protocol::{...}`
- `use crate::store::Store;` → `use crate::persistence::Store;`
- Test block:
  - `use crate::commands::{CommandCatalog, CommandEntry, OptionMatch, SubcommandMatch, ValueMatch};` → `use crate::catalog::{...}`
  - `use crate::engine::history::{description as history_description, sanitize_display};` → `use crate::providers::history::{...}`
  - `use crate::store::Store;` → `use crate::persistence::Store;`
- (`crate::config::{AcceptMode, Settings}` unchanged; `use super::*` test imports unchanged.)

### `src/providers/fuzzy.rs` (was `src/engine/fuzzy.rs`)
- `use crate::commands::CommandCatalog;` → `use crate::catalog::CommandCatalog;`
- `use crate::engine::history::candidate;` → `use crate::providers::history::candidate;`
- `use crate::protocol::{Candidate, CandidateKind, CandidateSource, CompletionResponse};` → `use crate::transport::protocol::{...}`
- `use crate::store::Store;` → `use crate::persistence::Store;`
- (`use super::MAX_BUFFER_BYTES;` unchanged; `use crate::config::Settings` unchanged.)

### `src/providers/filesystem.rs` (was `src/engine/filesystem.rs`)
- `use crate::protocol::{Candidate, CandidateKind, CandidateSource, CompletionResponse};` → `use crate::transport::protocol::{...}`
- (`use super::next_segment;` unchanged.)

### `src/providers/alias.rs` (was `src/engine/alias.rs`)
- `use crate::engine::history::sanitize_display;` → `use crate::providers::history::sanitize_display;`
- `use crate::protocol::{Alias, Candidate, CandidateKind, CandidateSource};` → `use crate::transport::protocol::{...}`
- Test block:
  - `use crate::commands::{CommandCatalog, CommandEntry, OptionMatch, SubcommandMatch};` → `use crate::catalog::{...}`
  - `use crate::engine::complete;` → `use crate::providers::complete;`
  - `use crate::store::Store;` → `use crate::persistence::Store;`
- (`use super::valid_command_prefix;` unchanged; `use crate::config::Settings` unchanged in tests.)

### `src/providers/history.rs` (was `src/engine/history.rs`)
- `use crate::protocol::{Candidate, CandidateKind, CandidateSource};` → `use crate::transport::protocol::{...}`
- `use crate::store::Store;` → `use crate::persistence::Store;`

### `src/providers/context.rs` (was `src/engine/context.rs`)
- No `crate::` changes (only `use super::valid_command_prefix;`).

### `src/catalog/` files (was `src/commands/`)
- No `crate::` changes needed — all intra-module references use `use super::` which remains valid because the parent is still one level up (`catalog/mod.rs`).
- (`use crate::clock::now_secs;` in `catalog/mod.rs` unchanged.)

### `src/persistence/mod.rs` (was `src/store.rs`)
- No `crate::` changes (uses only `std` + `rusqlite` + `anyhow`).

### Unchanged files (no `crate::` references)
- `src/clock.rs`, `src/config/mod.rs`, `src/preview/mod.rs`

---

## Visibility audit (`pub(super)` / `pub`)

All `pub(super)` items remain valid after the move because Rust's `pub(super)` = visible to the parent module and its descendants:

| Item | Current visibility | New location | Still valid? |
|---|---|---|---|
| `catalog::parse::{parse_*}` | `pub(super)` → visible to `catalog` | `catalog/parse.rs` | Yes, `catalog/mod.rs` is parent |
| `catalog::probe::discover_enrichment` | `pub(super)` → visible to `catalog` | `catalog/probe.rs` | Yes |
| `catalog::cache::{load_cache, save_cache, ...}` | `pub(super)` → visible to `catalog` | `catalog/cache.rs` | Yes |
| `providers::alias::{validate_aliases, ...}` | `pub(super)` → visible to `providers` | `providers/alias.rs` | Yes; `providers/mod.rs` is parent |
| `providers::context::{option_context, ...}` | `pub(super)` → visible to `providers` | `providers/context.rs` | Yes |
| `providers::history::{candidates, candidate, ...}` | `pub(super)` → visible to `providers` | `providers/history.rs | Yes; accessed from `providers/alias.rs` (descendant of `providers`) |
| `providers::filesystem::{filesystem_candidates, merge_filesystem_candidates}` | `pub` | `providers/filesystem.rs` | Yes |

No visibility changes required.

---

## Execution sequence

1. `git mv` all 19 files to new locations (in one commit or staged together).
2. Create `src/transport/mod.rs`.
3. Rewrite `src/lib.rs` module map.
4. Apply all import changes listed above.
5. Fix `include_str!` path in `src/shell/mod.rs`.
6. Run `cargo check --all-targets` — iterate on any remaining unresolved paths until clean.
7. Update `docs/ARCHITECTURE.md` "Source layout" listing to match the new tree.
8. Run full verification (see below).

---

## Verification criteria (must all pass)

- `cargo check --all-targets --all-features --locked`: zero errors
- `cargo fmt --all -- --check`: clean
- `cargo clippy --all-targets --all-features --locked -- -D warnings`: clean
- `cargo test --all-targets --all-features --locked`: 81 tests pass (79 lib + 2 bin), zero failures
- `cargo test --doc --locked`: pass
- `zsh -n -f src/shell/zsuggestion.zsh`: pass (template path unchanged on disk)
- `cargo package --locked`: pass
- Behavior identical: no logic altered, so test expectations unchanged

---

## Rollback
The branch `refactor/namespace-skeleton` is created from clean `ecf0975`. If any step fails, `git reset --hard ecf0975` returns to the verified baseline.

---

## Open questions before executing

1. **Scope**: Full rename (all namespaces above) vs conservative subset (only `store`→`persistence`, `engine`→`providers`, `commands`→`catalog`, leaving transport/shell/config/output/preview flat)?
2. **Module visibility**: Keep all top-level modules `pub mod` (current style) or tighten some to `mod` (private) now that they're nested? I recommend keeping `pub` to minimize API surface change.
3. **Alphabetical ordering in `lib.rs`**: I propose alphabetical (`catalog, clock, config, output, persistence, preview, providers, shell, transport`). Confirm or prefer grouping by layer?

Please confirm the scope and any naming preferences, and I'll execute.
