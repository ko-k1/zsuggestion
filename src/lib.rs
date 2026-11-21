//! zsuggestion: a command-first shell suggestion engine.
//!
//! The crate is split into a shared per-host daemon and thin shell clients:
//!
//! - [`catalog`]: installed-command inventory and lazy help/man enrichment.
//! - [`clock`]: wall-clock helpers shared by the CLI, daemon, and caches.
//! - [`cli`]: command-line argument parsing and subcommand dispatch.
//! - [`config`]: settings and filesystem locations.
//! - [`domain`]: tiered completion orchestration.
//! - [`output`]: completion response serialization formats.
//! - [`persistence`]: SQLite-backed shared history.
//! - [`preview`]: bounded file and command previews for the inline menu.
//! - [`shell`]: shell integration scripts emitted by `zsuggestion init`.
//! - [`transport`]: versioned JSON protocol over Unix socket (client + daemon).

pub mod catalog;
pub mod cli;
pub mod clock;
pub mod config;
pub mod domain;
pub mod output;
pub mod persistence;
pub mod preview;
pub mod shell;
pub mod transport;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
