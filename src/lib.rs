//! zsuggestion: a command-first shell suggestion engine.
//!
//! The crate is split into a shared per-host daemon and thin shell clients:
//!
//! - [`protocol`]: versioned request/response types exchanged over the socket.
//! - [`client`]: connects to (and lazily starts) the daemon.
//! - [`daemon`]: socket server that owns the [`store`] and [`commands`] catalog.
//! - [`engine`]: tiered candidate providers used by the daemon.
//! - [`commands`]: installed-command inventory and lazy help/man enrichment.
//! - [`store`]: SQLite-backed shared history.
//! - [`config`]: settings and filesystem locations.
//! - [`shell`]: shell integration scripts emitted by `zsuggestion init`.
//! - [`output`] and [`preview`]: CLI-side rendering for the shell frontend.

pub mod client;
pub mod clock;
pub mod commands;
pub mod config;
pub mod daemon;
pub mod engine;
pub mod output;
pub mod preview;
pub mod protocol;
pub mod shell;
pub mod store;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
