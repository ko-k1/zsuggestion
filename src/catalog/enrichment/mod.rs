//! Lazy, cached command enrichment.
//!
//! Unknown commands are queued for background workers that probe `man`/`--help`
//! (`probe`), parse the output (`parse`), and persist results in a
//! fingerprinted JSON cache (`cache`). Authored descriptions come from
//! `known`.

use std::sync::atomic::AtomicU64;

/// Shared counter for unique temporary-file names in probe and cache writes.
pub(crate) static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub mod cache;
pub mod known;
pub mod parse;
pub mod probe;
