# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Record user-visible behavior changes here; omit internal refactors, chores, and
documentation fixes that users cannot observe. Add new entries under
`Unreleased`, describe the outcome rather than the implementation, and move
entries into a dated version section when a release is tagged. Remove section
headings that have no entries for a release.

## [Unreleased]

### Added

- Repository documentation now includes a documentation index, vision, status,
  roadmap, architecture decision records, changelog, project policies, and
  generated agent guidance for the supported harnesses.

### Fixed

- The menu no longer stops at `max_candidates` rows of the first matching kind:
  directories, files, and history still appear after a full list of commands,
  subcommands, or options. History remains the bottom presentation tier and
  never appears ahead of regular suggestions.
- Native Zsh completions for nested paths (`ls src/…`) and option values
  (`--color=…`) are now shown, and native directories end in `/`.
- History no longer suggests paths that do not exist from the current
  directory (for example `cat notes.md` recorded elsewhere, or `./deploy.sh`),
  or commands that only ever failed with "command not found". Option values,
  chmod/chown operands, interpreter script arguments, and wrapped commands are
  not mistaken for paths. Entries containing shell syntax the tokenizer does
  not model, such as a trailing comment, are kept conservatively.
- History rows that duplicate a regular suggestion of any kind (command,
  option, subcommand, value, file, or directory, from the daemon or native
  Zsh completion) are replaced by the regular row, and remaining history stays
  in the bottom tier instead of mixing with regular suggestions. For commands
  whose operands are paths, `ls src` and `ls src/` count as the same row.
- For simple command lines, directory-only commands (`cd`, `pushd`, `mkdir`,
  `rmdir`) now suggest directories only, and `popd`/`dirs` no longer receive
  filesystem paths. A recorded `cd src` no longer repeats the filesystem's
  `cd src/` row.

[Unreleased]: https://github.com/ko-k1/zsuggestion/commits/main
