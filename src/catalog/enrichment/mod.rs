//! Lazy, cached command enrichment.
//!
//! Unknown commands are queued for background workers that probe `man`/`--help`
//! (`probe`), parse the output (`parse`), and persist results in a
//! fingerprinted JSON cache (`cache`). Authored descriptions come from
//! `known`.

pub mod cache;
pub mod known;
pub mod parse;
pub mod probe;
