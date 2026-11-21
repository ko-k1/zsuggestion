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
        +-- history provider
        +-- lazy man/help description workers
        +-- filesystem and dynamic providers (planned)
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

1. Parsed and cached root-command options, subcommands, and values.
2. Shell aliases, then installed commands and shell builtins for the first
   token.
3. Exact-prefix command history.
4. Bounded local filesystem matches at argument positions.
5. Append-safe candidates from the active Zsh completion system.
6. No result.

The rendered menu is re-sorted into presentation tiers before display:
installed commands first, then directories and files, then per-command options,
subcommands, and values, then remaining native candidates, with recorded
history always last so personal history never buries fresh command content.

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
  main.rs            thin clap CLI; maps subcommands to library calls
  lib.rs             crate root and module map
  protocol.rs        versioned request/response types
  client.rs          socket client; lazily starts the daemon
  daemon.rs          socket server, lock, request validation and dispatch
  engine/
    mod.rs           complete(): tiered providers, history, segment acceptance
    alias.rs         alias validation, alias candidates, leading-alias expansion
    context.rs       root-option, subcommand, and option-value contexts
    filesystem.rs    bounded path candidates merged behind higher tiers
    fuzzy.rs         fzf-ranked history and command search
  store.rs           SQLite history store and Zsh history import
  config.rs          strict TOML settings and XDG/override paths
  output.rs          JSON / NUL-separated completion output formats
  preview.rs         bounded file and allowlisted command previews
  shell.rs           renders the shell integration from its template
  shell/
    zsuggestion.zsh  Zsh frontend template
  commands/
    mod.rs           command inventory, prefix queries, worker queue
    probe.rs         sandboxed, time-bounded man / --help subprocesses
    parse.rs         conservative man/help parsers for options and subcommands
    cache.rs         fingerprinted, owner-private JSON enrichment cache
tests/
  zsh_ui.rs          end-to-end ZLE test driven through tmux
```

Edit the Zsh frontend in `src/shell/zsuggestion.zsh`; it is embedded with
`include_str!`, so a rebuild picks up changes. CI runs `zsh -n` on it, and a
unit test syntax-checks the rendered output.
