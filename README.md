# zsuggestion

zsuggestion is a natural shell completion system that puts command suggestions
first. It would rather show nothing than offer a completion it cannot justify.

## Overview

The project is in early development. The current vertical slice provides a
shared local daemon, SQLite-backed command history, Zsh history import, and
an inline completion menu, ghost text, command discovery, lazy descriptions,
native Zsh candidates, and full one-keystroke acceptance via Tab.

See [docs/VISION.md](docs/VISION.md) for purpose and principles, and
[docs/STATUS.md](docs/STATUS.md) for current implementation state and known
limits.

## Getting Started

### Build

```sh
cargo build --release
cargo install --path .
```

zsuggestion currently targets Unix systems because its shared transport is an
owner-private Unix socket. Inline fuzzy mode requires `fzf` on the host. The
crate is not yet published on crates.io; install from a checkout.

### Zsh Setup

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
- Uses Tab to fill the selected suggestion in one step and immediately queries
  for follow-up candidates. With no open suggestion, Tab delegates to the
  previous Zsh widget.
- Uses Shift-Tab to move upward through suggestions without entering a modal
  editing state; letters and Backspace continue editing normally.
- Consumes both consecutive trigger spaces to enter inline fuzzy mode over shared
  history and installed commands. Escape restores only the current command's
  space-free base buffer; Enter executes the selected result, while Ctrl-C and
  each new prompt discard all fuzzy state.
- Captures append-safe candidates from the Zsh completion system in a forked
  completion context after about 30-60 ms idle and blends them into the menu
  without blocking input. Like fzf-tab, it keeps each completion function's
  descriptions (`commit -- record changes to repository`) and maps its tags to
  `Sub`, `Opt`, `Value`, `File`, and `Dir` rows, so `git c`, `ls -`, and
  `git checkout ` list subcommands, flags, and branches. When `.zshrc` has not
  run `compinit`, zsuggestion loads it at the first prompt
  (`completion.compinit = false` opts out).
- Shows ranked candidates automatically as the command buffer changes.
- Accepts the ghost text zsh-autosuggestions style: Right, End, Ctrl-F, and
  Ctrl-E take the highlighted suggestion when the cursor is at the end of the
  line, and Alt-F or Ctrl-Right take its next word. Elsewhere those keys keep
  their previous widgets.
- Suggests your shell aliases alongside installed commands (`gst` shows
  `alias for git status`) and expands a leading alias before looking up flags,
  subcommands, and values, so `g ch` completes like `git ch`. Each request
  carries only the aliases that buffer needs; the shared daemon never stores
  shell state.
- Anchors the popup under the word being completed, like an IDE completion
  menu: zsuggestion asks the terminal where the prompt ends (falling back to
  `ui.prompt_offset` when the terminal does not answer), measures display
  width, rows show only the word being completed, and the popup shrinks to fit
  its rows up to `ui.menu_width`.
- Renders an nvim-style completion popup without window chrome: every row pairs
  its candidate with a dimmed description and a right-aligned kind badge
  (`Cmd`, `Hist`, `Native`, `File`, `Dir`, `Opt`, `Sub`, `Value`), the selected
  row carries a full-width background highlight, and a slim scrollbar
  (`█` thumb over a muted `│` track) appears on the right edge whenever
  candidates exceed the visible window. Long rows preserve the completion
  suffix, rendering as `… suggestion` when the typed command prefix would hide
  it.
- Adds a lazy preview pane at 100 columns or wider only when useful content is
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
- Uses Up and Down to move the popup selection while the menu is open, like an
  nvim completion menu; with no menu they fall back to the existing Zsh history
  widgets, cancelling stale completion work before history changes the buffer.
- Dismisses the popup with Escape without touching the command buffer, so a new
  query starts fresh the next time the buffer changes.
- Fills the full suggestion with Tab or the configured completion key; Enter
  remains the shell's untouched command-submission binding.
- Leaves tmux pane titles and automatic window naming entirely under tmux and
  the foreground application's control.

zsuggestion owns ZLE's suggestion display. Do not load a second autosuggestion
plugin alongside it; competing `POSTDISPLAY` highlights can recolor or stale
the menu.

Candidates are ranked in tiers. Installed commands come first, followed by
directory and file matches, then structured option, subcommand, and value
completions parsed from command metadata, then other native Zsh candidates.
Recorded history always sinks to the bottom of the menu regardless of which
tiers produced results. Explicit filesystem matches remain stable while native
results arrive, duplicate displays are removed, and equal-rank rows keep their
provider order.
At every argument position zsuggestion offers bounded
local filesystem matches, so path completion does not depend on a
command-specific completion function. In command position, an explicit path
(`./`, `../`, `/`, or `~/`) lists directories first, then executables, then
other files, so `./` suggests `./build/` and `./run.sh`. Root-command flags parsed from man pages
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
automatically falls back to ASCII markers, scrollbars, and separators otherwise.

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
compinit = true

[history]
ignore_leading_space = true
successful_first = true

[ui]
menu_width = 64
max_visible = 6
measure_prompt = true
prompt_offset = 2
accent = "14"
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
values. ANSI indexes follow the active terminal theme. With `measure_prompt`,
zsuggestion reads the prompt width from a terminal cursor-position report at
each new prompt; `prompt_offset` is the fallback visual width of the final
prompt line before editable text. The menu follows the
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

## Documentation

See [docs/INDEX.md](docs/INDEX.md). Release history is in
[CHANGELOG.md](CHANGELOG.md).

## Project Policies

- [Security](SECURITY.md)
- [Contributing](CONTRIBUTING.md)
- [Support](SUPPORT.md)
- [Code of Conduct](CODE_OF_CONDUCT.md)

## Agent Guidance

`AGENTS.md` is the canonical repository-wide entry point. The generator also
writes `CLAUDE.md`, `.agents/`, `.claude/skills/`,
`.github/copilot-instructions.md`, and `.github/skills/`.

Validate the generated configuration with:

```sh
python3 init-agents.py --check
```

`--init` is for initial generation in a fresh checkout; it refuses to overwrite
edited guidance files.
