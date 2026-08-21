# Roadmap

## Phase 1: Shared History Core

- Shared per-host daemon and SQLite state.
- Zsh command lifecycle capture.
- Existing history import without per-shell memory duplication.
- Exact-prefix ranking and segment acceptance.
- tmux and SSH as first-class operating environments.

## Phase 2: Help Discovery

- Cache installed executables from `PATH` and provide known command descriptions.
- Resolve the exact executable from `PATH`.
- Queue exact man-page and sandboxed `--help` inspection only for visible
  unresolved command candidates.
- Bound execution time, output bytes, stdin, pager, and color environment.
- Parse exact man `NAME` entries and conservative generic help summaries.
- Cache descriptions and misses by executable identity and timestamps.
- Parse common GNU, rendered man, Clap, Cobra, Click, and argparse flag layouts
  for root-command option candidates.
- Extend the cache key with subcommand paths.
- Add root subcommands and documented enum values.

## Phase 3: Interactive UI

- Bordered multiline ZLE suggestion menu and ghost text.
- Candidate kinds, descriptions, provenance, and scrolling viewport.
- Partial acceptance and immediate re-query.
- Debounced asynchronous requests with stale-result rejection.
- Preserve foreground-command behavior without taking ownership of tmux titles.

## Phase 4: Explicit Dynamic Providers

- Filesystem path segments at every argument position, with bounded scans,
  shell-safe escaping, and unambiguous Tab acceptance.
- Broaden native Zsh capture beyond append-safe candidates and preserve native
  display descriptions, quoting, suffix removal, and replacement ranges.
- Daemon-side executables, aliases, functions, and environment variable names.
- Git branches and remotes.
- Package scripts and build targets.
- Optional project-aware vocabulary provider.

Filesystem candidates are the conservative default at argument positions.
Other dynamic providers must be selected by a known argument type; Aster should
abstain when it cannot justify the candidate kind.

## Non-Goals

- Predicting broad command intent in the interactive path.
- Executing a suggestion automatically.
- Persisting command output or environment values.
- Requiring users to source completion code for every binary.
- Replacing native shell completion when Aster has no high-confidence result.
- Add PTY-owned Kitty image previews with explicit capability negotiation,
  image-ID cleanup, tmux passthrough, and sandboxed first-page PDF rasterization.
