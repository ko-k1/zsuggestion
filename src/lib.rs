//! zsuggestion: a command-first shell suggestion engine.
//!
//! The crate is split into a shared per-host daemon and thin shell clients:
//!
//! - [`catalog`]: installed-command inventory and lazy help/man enrichment.
//! - [`clock`]: wall-clock helpers shared by the CLI, daemon, and caches.
//! - [`config`]: settings and filesystem locations.
//! - [`output`]: completion response serialization formats.
//! - [`persistence`]: SQLite-backed shared history.
//! - [`preview`]: bounded file and command previews for the inline menu.
//! - [`providers`]: tiered candidate providers used by the daemon.
//! - [`shell`]: shell integration scripts emitted by `zsuggestion init`.
//! - [`transport`]: versioned JSON protocol over Unix socket (client + daemon).

pub mod catalog;
pub mod clock;
pub mod config;
pub mod output;
pub mod persistence;
pub mod preview;
pub mod providers;
pub mod shell;
pub mod transport;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
