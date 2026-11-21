# Roadmap

## Direction

Turn zsuggestion's conservative Zsh completion slice into a complete shell
completion system without giving up responsiveness or the rule that every
accepted candidate is justified by visible evidence.

## Next

- Nested subcommand metadata and typed positional arguments.
- Broaden native Zsh capture beyond append-safe candidates while preserving
  quoting, suffix removal, and replacement ranges.
- Daemon-side shell functions and environment variable names.
- Git branches and remotes.
- Package scripts and build targets.
- Optional project-aware vocabulary providers selected by a known argument type.
- Live configuration reload.

## Later

- A PTY frontend that owns Kitty image placement, image IDs, tmux passthrough,
  and cleanup.
- Sandboxed first-page PDF rasterization in that frontend.
- Additional dynamic providers once a known argument type can justify them.

Filesystem candidates are the conservative default at argument positions.
Other dynamic providers must be selected by a known argument type; zsuggestion
should abstain when it cannot justify the candidate kind.

## Non-goals

- Predicting broad command intent in the interactive path.
- Executing a suggestion automatically.
- Persisting command output or environment values.
- Requiring users to source completion code for every binary.
- Replacing native shell completion when zsuggestion has no high-confidence
  result.

Priorities may change as requirements and evidence change. Move completed work
to [STATUS.md](STATUS.md) rather than keeping a second status ledger here.
