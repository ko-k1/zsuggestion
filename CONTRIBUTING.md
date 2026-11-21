# Contributing

## Before Starting

- Read the project overview and the [documentation index](docs/INDEX.md).
- For substantial changes, discuss the proposed scope before implementation.
- Check existing issues and avoid duplicating active work.

## Development Setup

zsuggestion targets Unix systems and declares Rust 1.89 or newer in
`Cargo.toml`. The Zsh integration test also needs `zsh`; it uses `tmux` and
`fzf` for the full interactive path and skips itself when they are unavailable.

```sh
cargo build
cargo test
```

## Changes

- Keep each change focused and explain its purpose.
- Add or update tests and documentation when behavior changes.
- Run the checks relevant to the changed area.
- Do not include secrets, credentials, generated clutter, or unrelated changes.

## Checks

The CI workflow runs these commands on Linux and macOS:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
zsh -n -f src/shell/zsuggestion.zsh
cargo test --all-targets --all-features --locked
cargo test --doc --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked
cargo package --locked
python3 init-agents.py --check
```

`cargo package --locked` requires a clean worktree.

## Commits

Use one commit per concern with a subject in the form `type(scope): outcome`.
Type is one of `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `perf`, or
`ci`; scope names the package or area and is omitted for cross-cutting changes.
Write the subject in present tense, lower case, without a trailing period, and
explain what changed and why in the body.

Enable the repository commit template with:

```sh
git config commit.template .gitmessage
```

A user-visible behavior change also adds a `CHANGELOG.md` entry.

## Pull Requests

Describe the problem, the change, verification performed, and known
limitations. Keep the pull request focused and make sure the relevant checks
pass before requesting review.
