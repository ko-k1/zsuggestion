# zsuggestion

zsuggestion is a Zsh completion system that shows ranked suggestions as you
type — commands, flags, subcommands, values, paths, aliases, and history — in
a small popup under the word you're editing, like an editor's completion menu.

It needs no per-command setup: add one line to your `~/.zshrc` and it works
for every program you have installed.

## Install

```sh
cargo build --release
cargo install --path .
```

zsuggestion is not yet published on crates.io, so install from a checkout.
It targets Unix-like systems (Linux, macOS) and works best with `fzf`
installed for fuzzy search over your history.

## Set up

Add this to your `~/.zshrc`, then start a new shell:

```zsh
eval "$(zsuggestion init zsh)"
```

The background daemon starts automatically the first time you use it.

## How it feels

As you type, a menu appears and highlights the best suggestion:

| Key | What it does |
| :--- | :--- |
| `Tab` | Fill the highlighted suggestion in full |
| `Shift-Tab` | Move up through the menu |
| `Ctrl-Space` | Accept the highlighted candidate |
| `Right / End / Ctrl-F / Ctrl-E` | Take the ghost preview text |
| `Escape` | Dismiss the menu without changing your command |
| `Space Space` | Enter fuzzy search over history and commands |
| `Enter` | Run your command as usual |

Commands, flags, subcommands, values, files, and directories share the regular
suggestion tier, with commands listed first. Your command history is an
alternative at the bottom, and a matching history row gives way to the regular
suggestion. For simple command lines, commands that only accept directories,
such as `cd`, only show directory candidates. You never need completion scripts
for each tool.

## Commands

```text
zsuggestion daemon         start the background daemon
zsuggestion stop           stop the daemon
zsuggestion doctor         check paths and daemon connectivity
zsuggestion init zsh       print the one-line .zshrc integration
zsuggestion import-history --file ~/.zsh_history
zsuggestion record --command "git status" --cwd "$PWD" --exit-code 0
zsuggestion complete --buffer "git st" --cursor 6 --cwd "$PWD"
```

Client commands start the daemon automatically if it is not running.

## Configuration

`zsuggestion init zsh` writes a default config file. The most useful settings:

```toml
[completion]
max_candidates = 8   # candidate budget for each daemon tier
key = "ctrl-space"   # key that accepts a suggestion
accept = "full"      # fill the whole candidate on accept
compinit = true      # load Zsh's completion system if you haven't

[history]
ignore_leading_space = true
successful_first = true

[ui]
menu_width = 64
max_visible = 6
accent = "14"
```

Colors accept ANSI palette indexes `0`–`255` or `#RRGGBB`. Override paths with
`ZSUGGESTION_CONFIG`, `ZSUGGESTION_STATE_DIR`, `ZSUGGESTION_SOCKET`,
`XDG_CONFIG_HOME`, or `XDG_STATE_HOME`. Run `zsuggestion doctor` to see the
resolved paths.

## Learn more

- [docs/VISION.md](docs/VISION.md) — purpose and principles
- [docs/STATUS.md](docs/STATUS.md) — current state and known limits
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — how the daemon and shell fit together
- [docs/ROADMAP.md](docs/ROADMAP.md) — planned work

Project policies: [Security](SECURITY.md) ·
[Contributing](CONTRIBUTING.md) · [Support](SUPPORT.md) ·
[Code of Conduct](CODE_OF_CONDUCT.md)
