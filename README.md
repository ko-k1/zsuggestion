# zsuggestion

zsuggestion is a natural shell completion system that puts command suggestions
first. It would rather show nothing than offer a completion it cannot justify.

The project is in early development. The current vertical slice provides a
shared local daemon, SQLite-backed command history, Zsh history import, and
an inline completion menu, ghost text, command discovery, lazy descriptions,
native Zsh candidates, full acceptance, and segment-by-segment Tab completion.

## Principles

- Suggestions for installed commands outrank everything else; recorded history
  refines the menu instead of dominating it.
- Candidate providers are ordered tiers; lower tiers fill unused menu capacity
  but never outrank higher-confidence results.
- The menu appears while typing and highlights the highest-ranked candidate.
- Ctrl-Space accepts the entire candidate; Tab accepts one useful segment.
- No per-command shell completion scripts are required.
- Every host has one shared daemon and one history database.
- tmux panes and concurrent SSH shells remain lightweight clients.
- Silence is preferable to an uncertain completion.

## Build

```sh
cargo build --release
cargo install --path .
```

Or install the published package:

```sh
cargo install zsuggestion
```

zsuggestion currently targets Unix systems because its shared transport is an
owner-private Unix socket. Inline fuzzy mode requires `fzf` on the host.

## Zsh Setup

Add one integration line to `.zshrc`:

```zsh
eval "$(zsuggestion init zsh)"
```

This is zsuggestion's only shell integration. You do not need to source
completion scripts for every installed program.

The integration:

- Imports the existing Zsh history at shell startup when the file changed since
  the previous import.
- Records submitted foreground commands and the shell status reported afterward.
- Starts the per-host daemon automatically.
- Uses Tab to accept the shortest next word or path segment from an open
  zsuggestion menu; each accepted segment resets the refreshed menu to row 1.
  With no open suggestion, Tab delegates to the previous Zsh widget.
- Uses Shift-Tab to move upward through suggestions without entering a modal
  editing state; letters and Backspace continue editing normally.
- Consumes both consecutive trigger spaces to enter inline fuzzy mode over shared
  history and installed commands. Escape restores only the current command's
  space-free base buffer; Enter executes the selected result, while Ctrl-C and
  each new prompt discard all fuzzy state.
- Captures append-safe candidates from the configured Zsh completion system in
  a forked completion context after about 30-60 ms idle and blends them into
  the menu without blocking input.
- Shows ranked candidates automatically as the command buffer changes.
- Renders an nvim-style completion popup without window chrome: every row pairs
  its candidate with a dimmed description and a right-aligned kind badge
  (`Cmd`, `Hist`, `Native`, `File`, `Dir`, `Opt`, `Sub`, `Value`), the selected
  row carries a full-width background highlight, and a slim scrollbar
  (`█` thumb over a muted `┆` track) appears on the right edge whenever
  candidates exceed the visible window. Long rows preserve the completion
  suffix, rendering as `… suggestion` when the typed command prefix would hide
  it.
- Adds a lazy preview box at 100 columns or wider only when useful content is
  available. History-only and generic rows stay compact; command details reuse
  the existing asynchronous description workers, and selected native text files
  are read through a bounded, sanitized background helper. Changing rows erases
  the previous preview immediately and stale async results are target-checked.
- Previews simple `ls`, GNU `gls`, and `eza` suggestions asynchronously,
  including `ls` aliases backed by `eza`. zsuggestion passes a validated argv
  directly without a shell, translates `eza` colors into ZLE-safe highlight
  spans, disables icons and hyperlinks, caps output, and kills previews that
  exceed the short deadline.
- Uses Ctrl-Space to accept the entire highlighted candidate and preserves its
  previous binding as the fallback when zsuggestion has no candidate.
- Uses Ctrl-N and Ctrl-K to move through an open menu and updates inline ghost
  text to preview the selected row; outside the menu their prior widgets remain
  active.
- Leaves Up and Down dedicated to their existing Zsh history widgets, cancelling
  stale completion work before history changes the command buffer.
- Accepts the full suggestion with Ctrl-Space, or the shortest next segment with
  Tab; Enter remains the shell's untouched command-submission binding.
- Leaves tmux pane titles and automatic window naming entirely under tmux and
  the foreground application's control.

zsuggestion owns ZLE's suggestion display. Do not load a second autosuggestion
plugin alongside it; competing `POSTDISPLAY` highlights can recolor or stale
the menu.

Candidates are ranked in tiers. Structured option, subcommand, and value
completions parsed from command metadata come first, followed by the cached
inventory of installed commands. Recorded history fills the capacity those
command suggestions leave unused, explicit filesystem matches follow it and
remain stable while native results arrive, and asynchronous native Zsh
candidates close the menu. Duplicate displays are removed.
At every argument position zsuggestion offers bounded
local filesystem matches, so path completion does not depend on a
command-specific completion function. Root-command flags parsed from man pages
or sandboxed `--help` output appear lazily with their descriptions ahead of
duplicate native matches. Explicit command sections and documented option values
from the same metadata provide cached root subcommand and enum completions. Quoted
replacements, mid-word edits, and other non-append-safe completion behavior
remain under ordinary Zsh Tab. For ambiguous filesystem matches, Tab accepts
only their shared prefix and never selects the first entry arbitrarily. An exact
file remains selectable with a following Tab, which appends a space.

The same setup works inside tmux and on SSH hosts. Each remote host runs its own
daemon and keeps its own local history; tmux panes on that host share it.
zsuggestion uses its Unicode UI when `locale charmap` reports UTF-8 and
automatically falls back to ASCII borders and markers otherwise.

Image and PDF candidates currently show bounded metadata rather than graphics.
Kitty image transmission needs terminal-owned image IDs and cleanup that ZLE's
`POSTDISPLAY` model cannot provide safely; it is tracked for the future PTY UI.

## Conservative Acceptance

Given a history entry:

```text
cd ~/dev/gitrepos/zsuggestion
```

and the current buffer:

```text
cd ~/d
```

pressing Tab on the automatically highlighted candidate inserts only:

```text
ev/
```

zsuggestion then queries again from the new buffer. Pressing Ctrl-Space instead
inserts the complete `ev/gitrepos/zsuggestion` remainder. Tab also recognizes
structured shell values. For example, `ssh ali` completes to `ssh alice@` before
accepting the host, while `scp zzu` can advance through `zzuser@`,
`example.com:/`, and each remote path component separately. Assignments,
comma-separated values, URLs, rsync's `host::module` syntax, quoting, escaping,
and bracketed IPv6 hosts use the same conservative boundary scanner.

## Commands

```text
zsuggestion daemon
zsuggestion stop
zsuggestion doctor
zsuggestion init zsh
zsuggestion import-history --file ~/.zsh_history
zsuggestion record --command "git status" --cwd "$PWD" --exit-code 0
zsuggestion complete --buffer "git st" --cursor 6 --cwd "$PWD"
```

Client commands automatically start the daemon if it is unavailable.

## Configuration

The default configuration is written by `zsuggestion init zsh`:

```toml
[completion]
max_candidates = 8
key = "ctrl-space"
accept = "full"

[history]
ignore_leading_space = true
successful_first = true

[ui]
menu_width = 64
max_visible = 6
prompt_offset = 2
border = "4"
accent = "10"
text = "7"
muted = "8"
ghost = "8"
selected_background = "8"
selected_text = "15"
selected_source = "0"
```

`completion.key` also accepts Ctrl-letter names such as `ctrl-x`. Ctrl-I (Tab),
Ctrl-J, Ctrl-K, Ctrl-M, and Ctrl-N are reserved by the menu integration. The
generated binding is refreshed when a new shell starts.
UI colors accept ANSI palette indexes from `0` through `255` or exact `#RRGGBB`
values. ANSI indexes follow the active terminal theme. `prompt_offset` is the
visual width of the final prompt line before editable text. The menu follows the
live letter cursor and clamps before the terminal's final column.

Paths can be overridden for testing or custom deployments:

| Variable | Purpose |
| :------- | :------ |
| `ZSUGGESTION_CONFIG` | Configuration file |
| `ZSUGGESTION_STATE_DIR` | SQLite database and daemon log directory |
| `ZSUGGESTION_SOCKET` | Unix socket path |
| `XDG_CONFIG_HOME` | Default config root |
| `XDG_STATE_HOME` | Default state root |

Run `zsuggestion doctor` to print the resolved paths and verify daemon
connectivity. Custom state and socket paths must live beneath directories owned
by the current user and inaccessible to group and other users.

## Status

Implemented:

- Shared auto-started daemon.
- Versioned JSON protocol over an owner-private Unix socket.
- SQLite WAL history shared by all shells on a host.
- Change-detected Zsh history import.
- Recency-first history ranking, with current directory, success, and usage as
  tie-breakers.
- Cached `PATH` command inventory, shell builtins, semantic command icons, and
  authored descriptions for common tools.
- Lazy command-description enrichment from exact man pages and sandboxed
  `--help` output, with executable-fingerprinted persistent caching.
- Cached root subcommands and documented option values parsed from explicit help
  sections, including common Clap, Cobra, Click, and argparse layouts.
- Asynchronous native Zsh candidate capture for append-safe options,
  subcommands, paths, aliases, and other configured completion sources.
- Prefix-only completion with conservative partial acceptance.
- Cursor-anchored, scrolling ZLE popup with ghost text, descriptions, kind
  badges, highlighted matches, a position scrollbar, and configurable colors.
- Debounced asynchronous completion requests that never block character input.
- Zsh native-completion fallback.

Description discovery never runs on the completion request path. An unresolved
visible command keeps its origin fallback, is queued on a bounded worker pool,
and is refreshed in the menu when metadata becomes ready. zsuggestion checks
the exact man page first. On macOS, it may then run `--help` through
`sandbox-exec` with network and writes denied, an empty environment, bounded
output, and a hard timeout. Direct executable probing is disabled on platforms
without that sandbox. Cached misses expire after one day; descriptions expire
after 30 days or immediately when the executable path, identity, size, mode, or
timestamps change.

Planned:

- Nested subcommand metadata and typed positional arguments.
- Optional project-aware providers.

See [`docs/architecture.md`](docs/architecture.md) and
[`docs/roadmap.md`](docs/roadmap.md).
