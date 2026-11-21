//! Recorded-history candidates.
//!
//! History is always the lowest presentation tier. Candidates carry the
//! complete suffix because the frontend replaces the typed baseline in one
//! step; an exact history match is not a useful completion and is skipped.

use anyhow::Result;

use crate::persistence::Store;
use crate::transport::protocol::{Candidate, CandidateKind, CandidateSource};

pub(crate) fn candidates(
    store: &Store,
    prefix: &str,
    cwd: &str,
    limit: usize,
    successful_first: bool,
) -> Result<Vec<Candidate>> {
    let mut candidates = Vec::new();
    for history in store.history_candidates(prefix, cwd, limit, successful_first)? {
        let Some(suffix) = history.command.strip_prefix(prefix) else {
            continue;
        };
        if suffix.is_empty() {
            continue;
        }
        let candidate = Candidate {
            display: sanitize_display(&history.command),
            description: description(history.uses, history.same_cwd),
            description_pending: false,
            kind: CandidateKind::History,
            insert_text: suffix.to_owned(),
            // `complete` overrides this for segment acceptance; full-line
            // acceptance re-inserts the suffix.
            accept_text: suffix.to_owned(),
            source: CandidateSource::History,
        };
        if candidates
            .iter()
            .any(|existing: &Candidate| existing.display == candidate.display)
        {
            continue;
        }
        candidates.push(candidate);
        if candidates.len() == limit {
            break;
        }
    }
    Ok(candidates)
}

/// Builds an unsuffixed history candidate for fuzzy mode, where the query
/// replaces the whole line rather than appending to a typed prefix.
pub(crate) fn candidate(command: &str, uses: usize, same_cwd: bool) -> Candidate {
    Candidate {
        display: sanitize_display(command),
        description: description(uses, same_cwd),
        description_pending: false,
        kind: CandidateKind::History,
        insert_text: command.to_owned(),
        accept_text: command.to_owned(),
        source: CandidateSource::History,
    }
}

pub(crate) fn description(uses: usize, same_cwd: bool) -> String {
    match (uses, same_cwd) {
        (1, true) => "used here".to_owned(),
        (1, false) => "used once".to_owned(),
        (uses, true) => format!("used {uses}x, here"),
        (uses, false) => format!("used {uses}x"),
    }
}

pub(crate) fn sanitize_display(value: &str) -> String {
    let mut display = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_control() {
            display.extend(character.escape_default());
        } else {
            display.push(character);
        }
    }
    display
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::Store;

    #[test]
    fn describes_history_usage_and_directory() {
        assert_eq!(description(1, true), "used here");
        assert_eq!(description(3, true), "used 3x, here");
        assert_eq!(description(2, false), "used 2x");
    }

    #[test]
    fn escapes_control_characters_in_display_text() {
        assert_eq!(sanitize_display("echo\t\u{1b}"), "echo\\t\\u{1b}");
    }

    #[test]
    fn skips_exact_history_matches() {
        let store = Store::in_memory().unwrap();
        store
            .record("git status", "/repo", 0, 100, "test", true)
            .unwrap();
        let candidates = candidates(&store, "git status", "/repo", 10, true).unwrap();
        assert!(candidates.is_empty());
    }
}
