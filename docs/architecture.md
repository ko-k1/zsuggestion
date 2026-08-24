# Architecture

## Goals

zsuggestion must remain correct and responsive with many shells open across
tmux panes and SSH sessions. Shell processes do not load the history corpus.
They send small requests to one daemon on the current host.

```text
Zsh / future PTY UI
        |
        +-- forked native completion capture
        |
        | versioned request over Unix socket
        v
   zsuggestion daemon
        |
        +-- structured help and command inventory (highest priority)
        +-- history provider
        +-- lazy man/help description workers
        +-- filesystem and dynamic providers (planned)
        |
        v
  SQLite shared state
```

## Shared State

The daemon owns a primary SQLite connection configured for WAL mode and a
bounded busy timeout. History imports use a separate short-lived connection so
file parsing and bulk insertion do not hold the completion-store mutex. All tmux
panes on a host use the same socket and database. SSH sessions use the daemon on
the remote host, avoiding accidental transfer of command history between
machines.

Command descriptions, parsed root options, subcommands, and documented option
values use a separate owner-private JSON cache. Entries are keyed by resolved
executable path, device, inode, size, mode, modification time, and change time.
This keeps cache loading and persistence independent of the history database and
its completion lock.

The shell sends only command text, working directory, timestamp, session ID, and
exit code. zsuggestion does not collect stdout, stderr, environment values, or command
dependency graphs.

Imported history is tracked by canonical path, size, and modification time. An
unchanged history file is not reparsed when another shell starts.
History candidates are ordered by their latest observation first. Same-directory
use, successful exits, frequency, and command text provide deterministic
tie-breakers in that order.

## Completion Tiers

Providers are queried in strict order:

1. Parsed and cached root-command options, subcommands, and values.
2. Cached installed-command inventory and known shell builtins for the first
   token.
3. Exact-prefix command history.
4. Bounded local filesystem matches at argument positions.
5. Append-safe candidates from the active Zsh completion system.
6. No result.

Lower tiers may fill unused menu capacity but never outrank a higher tier. No
broad command-intent model is in the interactive path.

## Conservative Editing

Every response includes:

- The complete display candidate.
- A short description and semantic candidate kind.
- The complete insertion suffix.
- The smaller text accepted by one completion-key press.
- The byte range the frontend may replace.
- Candidate provenance.

The initial engine only completes at the end of the buffer. It abstains from
mid-line edits until a shell-aware parser can produce safe replacement ranges.

The Zsh frontend refreshes an inline candidate popup when the command buffer
changes and highlights the highest-ranked candidate. It owns `POSTDISPLAY` while
suggestions are active, so the ghost text and borderless popup participate in
ZLE's normal multiline layout instead of being printed into terminal history.
ZLE `region_highlight` entries style matches, provenance, descriptions, kind
badges, the scrollbar, and the full-width selected-row background using
validated colors from the UI configuration. Arrow keys, Ctrl-N, and Ctrl-K move
the selection; Shift-Tab cycles it upward; Escape dismisses the popup without
touching the buffer; Tab fills the selected candidate in full while the menu is
open — appending only the text after the query baseline so an existing prefix is
never duplicated — adds a trailing space for command-like results, and
immediately queries again for follow-up arguments. The configured completion
key accepts the same way.
Selection does not change the
editing keymap, so character insertion and Backspace keep their ordinary
behavior. Enter is never rebound. With no menu, Tab and Shift-Tab clear stale
display state and delegate to the widgets they replaced. Native completion entry
is serialized so the asynchronous ticker cannot recursively enter Zsh completion.
Up and Down move the popup selection while the menu is open; with no menu they
delegate to the history widgets after cancelling menu requests, preventing stale
callbacks from restoring the pre-history buffer.
The integration snapshots native widgets only on first load, but reinstalls its
functions, hooks, and bindings on every evaluation so sourcing `.zshrc` remains
safe after `compinit` or other ZLE plugins recreate their wrappers.

Two consecutive spaces enter an explicit fuzzy mode and both trigger spaces are
removed before saving the fuzzy base. Query text is mirrored into `BUFFER` so
ZLE keeps its real cursor after the query. `POSTDISPLAY` shows
the selected fuzzy result as ghost text and changes immediately with menu
navigation. Each query update asynchronously asks the daemon for a bounded
history and command inventory ranked by `fzf --filter`. Escape restores the
saved base and cancels the request. Enter accepts the selected candidate and
delegates to the original command-submission widget. Ctrl-C clears fuzzy
base/query state before delegating to the shell interrupt, and `line-init` resets
that state again for every new prompt.

zsuggestion memo-tags only its own highlight regions. It preserves foreign regions
across asynchronous FD callbacks and restores an immutable in-flight buffer
snapshot before redraw. Results are discarded when that snapshot no longer
matches the live editing buffer, so stale completion cannot erase typing or
pasted text and syntax-highlighting plugins retain command-token colors.

The menu is anchored from a configurable prompt offset plus the live ZLE cursor
and clamped before the terminal's final column. It renders a bounded
viewport with scrolloff, a position counter, semantic icons, descriptions, and
key hints; the candidate order always remains the daemon's ranking order. Rows
that exceed the title width are left-truncated: when the completion remainder
fits, the row is rendered as `… remainder`, otherwise its distinguishing tail is
preserved.

Character-editing widgets only update a request snapshot and dirty flag. A
prompt-lifetime ZLE ticker debounces those changes and launches completion in a
background process; an fd handler discards superseded work and asks ZLE to apply
only the current response. No process launch, database access, or socket round
trip occurs on the character-input path. While replacement results are pending,
append edits retain and rebase visible candidates that still match the buffer;
the menu clears immediately only when every displayed candidate is stale.

Unresolved command metadata is also excluded from that path. A completion
request performs only an in-memory cache lookup and nonblocking bounded enqueue.
Two daemon workers inspect only visible commands, preferring the
exact man page before a direct `--help` probe. Man and help processes have
owner-private bounded output, process-group timeouts, and cleared pager/color
state. On macOS, direct executable probing additionally runs through
`sandbox-exec` with network and writes denied; platforms without an implemented
sandbox use man pages only. Parsers retain a bounded set of strict ASCII option
spellings and control-free descriptions from recognized option sections; usage
examples and free prose never become candidates.

Responses report whether metadata discovery is pending even when no candidate is
available yet. Zsh polls the unchanged request about every 150 ms, only one
request at a time, until the workers settle it. Polling preserves the numeric
menu row and stops immediately when enrichment settles. Legacy five- and
six-field Zsh streams remain available; newly initialized shells use a versioned
stream header followed by six-field candidate records.

The daemon scans at most 1,024 directory entries for the current unquoted path
token after any command-space boundary. It resolves relative paths against the
request cwd, supports `~/` and backslash-escaped path components, hides dotfiles
until a dot prefix is typed, escapes shell-sensitive insertion characters, and
skips non-file special entries. Directory reads happen after releasing the
history-store mutex. When history fills the configured limit, the merger reserves
up to half the menu for filesystem candidates so paths remain available.

Native Zsh completions are shell-local and therefore never generated by the
daemon. After about 30-60 ms idle on a changed buffer, zsuggestion invokes a completion
widget that immediately forks a child completion context. The child runs the
shell's configured `_main_complete`, wraps ordinary `compadd` calls, and returns
a bounded list over a private pipe. The parent ZLE does not wait for completion
functions or their subprocesses. A new edit closes the pipe and cancels the
child; a 1.5-second ticker deadline handles completion functions that do not
finish.

The frontend accepts only literal append operations at end-of-line. Matches
that need quoting, hidden prefixes, ignored prefixes, in-word replacement, or
other native edit semantics are discarded, while Tab with no open zsuggestion menu
remains delegated to Zsh for exact behavior. Frontend merging preserves the
daemon's ranked order — structured help, command inventory, history, filesystem
matches — then appends native matches. It deduplicates by full display text and
preserves the current numeric row across asynchronous updates, so newly inserted
providers cannot move row 1 to a seemingly random position.

At shell initialization, the frontend checks `locale charmap`. UTF-8 locales use
the borderless glyph UI; other locales use single-byte ASCII fallbacks, markers,
icons, separators, and truncation characters so ZLE does not expose encoded
bytes as `<e2><...>` sequences in SSH or container sessions.

At 100 columns or wider, the selected candidate gets a borderless preview pane
only after useful content exists. History and generic metadata never reserve preview
space. Command preview text follows the existing lazy description lifecycle. Native
and explicit filesystem regular-file candidates use a separate bounded helper
that reads at most 16 KiB,
rejects symlinks and non-regular files, and strips non-ASCII controls. Image and
PDF targets expose metadata only. Graphical Kitty output is reserved for a PTY
frontend that can own image placement, IDs, tmux passthrough, and cleanup.
Each preview response carries the selected target identity; switching rows clears
the old lines before scheduling new work, and mismatched responses are discarded.

Read-only command previews use an explicit provider whitelist. The initial
provider recognizes only simple `ls`, GNU `gls`, and `eza` command lines. An
`ls` alias whose executable is `eza` is translated before validation. The
provider rejects shell syntax and alternate executable paths, invokes the
program directly without a shell, disables icons and hyperlinks, bounds captured
output, and terminates the process group after 600 ms. ANSI SGR sequences from
`eza` are parsed into validated `region_highlight` spans so color is retained
without placing terminal control bytes in `POSTDISPLAY`. Other commands remain
description-only until they receive a separately reviewed provider.

## tmux Titles

zsuggestion never changes `pane_title`, window names, `automatic-rename`, or status
formats. Those values remain under tmux and the foreground application's control.
The Zsh hooks use `TMUX_PANE` only as part of the history session identifier.

## Daemon Lifecycle

Client commands first attempt the Unix socket. When unavailable, the client
starts `zsuggestion daemon` in a new process session and waits for a successful ping.
Concurrent startup attempts are serialized by an owner-private lock held for
the daemon lifetime. Only the lock owner may inspect or remove a stale socket
and bind the endpoint.

The socket and state directories are owner-only. A stale socket left by an
unclean shutdown is removed only after a connection attempt proves no daemon is
listening.

## Configuration

Configuration is strict TOML with unknown fields rejected. Paths follow XDG
locations by default and have explicit `ASTER_*` overrides. The initial daemon
loads configuration at startup; live config reload is planned.
