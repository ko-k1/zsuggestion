# Architecture

## Goals

zsuggestion stays correct and responsive with many shells open across tmux
panes and SSH sessions. Shell processes never load the history corpus; they
send small requests to one daemon on the current host.

```text
Zsh / future PTY UI
        |
        +-- forked native completion capture
        |
        | versioned request over Unix socket
        v
   zsuggestion daemon
        |
        +-- command inventory and parsed help (highest priority)
        +-- recorded history
        +-- bounded filesystem matches
        +-- lazy man/help description workers
        +-- additional dynamic providers (planned)
        |
        v
   SQLite shared state
```

## Shared state

The daemon owns one SQLite connection in WAL mode with a bounded busy timeout.
History imports use a separate short-lived connection so file parsing and bulk
insertion never hold the completion lock. All tmux panes on a host share the
same socket and database; SSH sessions talk to the daemon on the remote host,
so command history never leaks between machines.

Command descriptions, parsed root options, subcommands, and documented option
values live in a separate owner-private JSON cache keyed by executable identity.
Cache misses expire after one day; descriptions expire after 30 days or when
the executable changes. Cache loading is independent of the history database
and its completion lock.

The shell sends only command text, working directory, timestamp, session ID,
and exit code. zsuggestion never collects stdout, stderr, environment values,
or command dependency graphs.

Imported history is tracked by canonical path, size, and modification time, so
an unchanged history file is not re-parsed on every shell start. History
candidates are ordered by latest observation first, with same-directory use,
successful exits, frequency, and command text as deterministic tie-breakers.

## Completion tiers

Providers are queried in strict order:

1. Shell aliases, then installed commands and shell builtins for the command
   word.
2. Parsed and cached root-command options, subcommands, and values for the
   current command context.
3. Bounded local filesystem matches at argument positions. For simple command
   lines, commands whose operands must be directories (`cd`, `pushd`, `mkdir`,
   `rmdir`) receive directories only, and commands that take no paths (`popd`,
   `dirs`) receive none. Compound commands, assignments, and wrappers fall
   back to all candidates because their first word no longer describes the
   command at the cursor.
4. Exact-prefix command history, skipping entries whose untyped words name
   paths that do not exist from the current directory, entries whose directory
   operand is an existing non-directory, duplicate directory rows already
   supplied by the filesystem tier for path-taking commands, and commands that
   only ever failed as not found (exit 126/127). Entries containing unmodelled
   shell syntax are kept conservatively.
5. Append-safe candidates from the active Zsh completion system.
6. No result.

The rendered menu keeps provider order within the regular suggestion tier:
installed commands, then per-command options, subcommands, and values, then
directories and files, followed by remaining native candidates. Recorded
history is always last, so personal history never buries current command
content. A history row that duplicates a regular suggestion of any kind,
including a native one, is replaced by the regular row. Duplicates ignore
trailing whitespace and, for commands whose operands are paths, a trailing
`/`; other commands (such as `git checkout`) keep `src` and `src/` distinct.

Shell aliases are shell-local, so the frontend sends them with each request
rather than registering them with the shared daemon. While the command word is
being typed it sends at most 32 aliases whose names extend that word; once
arguments begin it sends only the leading alias chain (up to eight links,
stopping at cycles). The daemon offers matching aliases as `Cmd` rows ahead of
the executables they shadow, and expands the leading alias before resolving
options, subcommands, and values. Global and suffix aliases are not expanded.

## Daemon lifecycle

Client commands first attempt the Unix socket. When unavailable, the client
starts `zsuggestion daemon` in a new process session and waits for a
successful ping. Concurrent startup attempts are serialized by an owner-private
lock held for the daemon's lifetime; only the lock owner may inspect or remove
a stale socket and bind the endpoint.

The socket and state directories are owner-only. A stale socket left by an
unclean shutdown is removed only after a connection attempt proves no daemon
is listening.

## Configuration

Configuration is strict TOML with unknown fields rejected. Paths follow XDG
locations by default and can be overridden with
`ZSUGGESTION_CONFIG`, `ZSUGGESTION_STATE_DIR`, and `ZSUGGESTION_SOCKET`. The
daemon loads configuration at startup; live reload is planned.

## Source layout

```text
src/
  main.rs            thin CLI entry point; parses and dispatches to cli::run
  lib.rs             crate root and module map
  cli/               clap definitions, request building, response handling
    mod.rs
  clock/             wall-clock helpers shared by the CLI, daemon, and caches
    mod.rs
  config/            strict TOML settings and XDG/override paths
    mod.rs           Settings, Paths, defaults, validation
  output/            JSON / NUL-separated completion output formats
    mod.rs           OutputFormat, write_completion
  preview/           bounded file and allowlisted command previews
    mod.rs           write_file_preview, write_command_preview
  shell/             renders the shell integration from its template
    mod.rs           ZSH_TEMPLATE and placeholder substitution
    zsuggestion.zsh  Zsh frontend template
  transport/         versioned JSON protocol over Unix socket
    mod.rs           client, daemon, protocol module map
    client.rs        socket client; lazily starts the daemon
    daemon.rs        socket server, lock, request validation and dispatch
    protocol.rs      versioned request/response types
  catalog/           command inventory and lazy help/man enrichment
    mod.rs           CommandCatalog, prefix queries, enrichment state
    inventory.rs     PATH discovery, cache hydration, background workers
    enrichment/      help/man probing, parsing, and persistence
      mod.rs
      known.rs       authored descriptions and shell builtins
      parse.rs       conservative man/help parsers
      probe.rs       sandboxed, time-bounded man / --help subprocesses
      cache.rs       fingerprinted, owner-private JSON enrichment cache
  domain/            completion orchestration
    mod.rs           complete(), fuzzy(), request validation, tier merging
    completion/      tiered candidate providers
      mod.rs         command, structured, and history tiers
      context.rs     root-option, subcommand, and option-value contexts
      segments.rs    segment acceptance for history candidates
      tiers/
        aliases.rs   alias validation, candidates, leading-alias expansion
        filesystem.rs bounded path candidates
        history.rs   recorded-history candidates and path plausibility
        paths.rs     shared command/path classification
    fuzzy/           fzf-ranked history and command search (Space Space)
                     and history-only search (Ctrl-R)
      mod.rs
  persistence/       SQLite history store and Zsh history import
    mod.rs           Store, HistoryCandidate, ImportResult, import
test/
  zle_integration.rs end-to-end ZLE test driven through tmux
```

Edit the Zsh frontend in `src/shell/zsuggestion.zsh`; it is embedded with
`include_str!`, so a rebuild picks up changes. CI runs `zsh -n` on it, and a
unit test syntax-checks the rendered output.
