# Vision

## Purpose

zsuggestion exists to make interactive shell completion feel like an editor
completion menu: relevant, ranked, and safe to accept. It should offer the next
command, flag, subcommand, value, path, or remembered command without requiring
a separate completion script for every installed program.

## Problem

Traditional shell completion is fragmented. Each command ships its own scripts,
coverage varies, and mixed sources can bury the most useful candidate. Users
often type more than necessary or fall back to history search. zsuggestion
centralizes discovery and ranking behind one local daemon while keeping the
shell responsive.

## Goals

- Complete commands, options, subcommands, documented values, paths, aliases,
  and history from one consistent menu.
- Keep the character-input path free of database access, process launches, and
  socket round trips.
- Share one daemon and one history database per host so tmux panes and SSH
  sessions remain lightweight.
- Accept only text the user can see and justify; when confidence is low,
  delegate to native Zsh completion or show nothing.
- Keep the Zsh integration to one line and require no per-command setup.
- Preserve owner-private state and avoid collecting command output or
  environment values.

## Non-goals

- Predicting broad command intent in the interactive path.
- Executing a suggestion automatically.
- Persisting command output, environment values, or dependency graphs.
- Replacing native shell completion when zsuggestion has no high-confidence
  result.
- Owning tmux pane titles or window naming.
- Rendering images or PDFs inside the ZLE popup; graphics belong to a future
  PTY frontend.

## Principles

- Command first: installed commands are offered before any other tier, mirroring
  ghost-complete. Per-command flags, subcommands, and values follow, then
  filesystem matches, and recorded history always sinks to the bottom of the
  menu.
- Candidate providers are ordered tiers; lower tiers fill unused menu capacity
  but never outrank higher-confidence results.
- The menu appears while typing and highlights the highest-ranked candidate.
- Tab fills the entire selected candidate; the configured completion key
  (default Ctrl-Space) accepts it as well.
- No per-command shell completion scripts are required.
- Every host has one shared daemon and one history database.
- tmux panes and concurrent SSH shells remain lightweight clients.
- Silence is preferable to an uncertain completion.

## Success Criteria

- A new user can install zsuggestion, add one line to `.zshrc`, and receive
  ranked suggestions without per-command completion scripts.
- Typing remains responsive with many tmux panes and concurrent SSH sessions
  because shell processes never load the history corpus.
- Every accepted candidate inserts exactly the displayed text and nothing the
  user cannot see.
- When no provider can justify a candidate, the menu stays silent and the
  previous Zsh behavior remains available.
- The daemon socket and state directories remain owner-private and local to the
  host.
