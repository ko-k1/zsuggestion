# Vision

## Purpose

zsuggestion makes interactive shell completion feel like an editor's
completion menu: relevant, ranked, and safe to accept. It suggests the next
command, flag, subcommand, value, path, alias, or remembered command — without
a separate completion script for every program you install.

## Problem

Traditional shell completion is fragmented. Each command ships its own scripts,
coverage varies, and mixed sources bury the most useful candidate. Users type
more than necessary or fall back to history search. zsuggestion centralizes
discovery and ranking behind one local daemon while keeping the shell
responsive.

## Goals

- Complete commands, options, subcommands, documented values, paths, aliases,
  and history from one consistent menu.
- Keep typing free of database access, process launches, and socket round trips.
- Share one daemon and one history database per host so tmux panes and SSH
  sessions stay lightweight.
- Accept only text the user can see and justify; when confidence is low,
  delegate to native Zsh completion or show nothing.
- One line in `.zshrc`; no per-command setup.
- Keep state owner-private; never collect command output, environment values,
  or dependency graphs.

## Non-goals

- Predicting broad command intent while you type.
- Executing a suggestion automatically.
- Persisting command output, environment values, or dependency graphs.
- Replacing native shell completion when zsuggestion has no high-confidence
  result.
- Owning tmux pane titles or window naming.
- Rendering images or PDFs inside the ZLE popup; graphics belong to a future
  PTY frontend.

## Principles

- Commands, flags, subcommands, values, and filesystem matches share the
  regular suggestion tier, with commands listed first; history is an
  alternative at the bottom.
- Candidate providers retain their order within the regular tier, and history
  never outranks a regular suggestion or crowds one out.
- The menu appears while typing and highlights the highest-ranked candidate.
- Tab fills the whole selected candidate; the configured key (default
  Ctrl-Space) accepts it too.
- No per-command shell completion scripts required.
- One shared daemon and one history database per host.
- tmux panes and concurrent SSH shells remain lightweight clients.
- Silence is preferable to an uncertain completion.

## Success criteria

- A new user installs zsuggestion, adds one line to `.zshrc`, and receives
  ranked suggestions without per-command completion scripts.
- Typing stays responsive with many tmux panes and concurrent SSH sessions
  because shell processes never load the history corpus.
- Every accepted candidate inserts exactly the displayed text and nothing the
  user cannot see.
- When no provider can justify a candidate, the menu stays silent and the
  previous Zsh behavior remains available.
- The daemon socket and state directories stay owner-private and local to the
  host.
