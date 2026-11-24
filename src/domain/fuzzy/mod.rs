//! Inline fuzzy mode: ranks shared history and installed commands with `fzf`.
//!
//! `Space Space` searches history and commands; `Ctrl-R` searches history
//! only. Without `fzf` on `PATH`, a built-in matcher keeps history order and
//! requires every space-separated query term to appear, ignoring case.

use std::collections::HashSet;
use std::io::ErrorKind;
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;

use anyhow::{Result, bail};

use super::MAX_BUFFER_BYTES;
use crate::catalog::CommandCatalog;
use crate::config::Settings;
use crate::domain::completion::tiers::history::candidate;
use crate::persistence::Store;
use crate::transport::protocol::{
    Candidate, CandidateKind, CandidateSource, CompletionResponse, FuzzyScope,
};

const FUZZY_HISTORY_LIMIT: usize = 4096;
/// Rows returned by history search: enough to scroll like fzf's history
/// widget, and within the frontend's menu budget for any configuration.
pub const HISTORY_SEARCH_LIMIT: usize = 64;

pub fn fuzzy(
    store: &Store,
    commands: &CommandCatalog,
    query: &str,
    cwd: &str,
    requested_limit: Option<usize>,
    scope: FuzzyScope,
    settings: &Settings,
) -> Result<CompletionResponse> {
    if query.len() > MAX_BUFFER_BYTES {
        bail!("fuzzy query exceeds {MAX_BUFFER_BYTES} bytes");
    }
    let budget = match scope {
        FuzzyScope::All => settings.completion.max_candidates,
        FuzzyScope::History => HISTORY_SEARCH_LIMIT,
    };
    let limit = requested_limit.unwrap_or(budget).min(budget);
    let mut seen = HashSet::new();
    let mut pool = Vec::new();

    for history in
        store.history_inventory(cwd, FUZZY_HISTORY_LIMIT, settings.history.successful_first)?
    {
        if seen.insert(history.command.clone()) {
            pool.push(candidate(&history.command, history.uses, history.same_cwd));
        }
    }
    let commands_pool = match scope {
        FuzzyScope::All => commands.inventory(),
        FuzzyScope::History => Vec::new(),
    };
    for command in commands_pool {
        if seen.insert(command.name.clone()) {
            pool.push(Candidate {
                display: command.name.clone(),
                description: command.description,
                description_pending: false,
                kind: CandidateKind::Command,
                insert_text: command.name.clone(),
                accept_text: command.name,
                source: CandidateSource::Command,
            });
        }
    }

    let indexes = fzf_indexes(&pool, query, limit)?;
    let mut candidates = Vec::with_capacity(indexes.len());
    for index in indexes {
        let mut candidate = pool[index].clone();
        if candidate.source == CandidateSource::Command
            && let Some(command) = commands
                .matching(&candidate.display, 1)
                .into_iter()
                .find(|command| command.name == candidate.display)
        {
            candidate.description = command.description;
            candidate.description_pending = command.description_pending;
        }
        candidates.push(candidate);
    }
    Ok(CompletionResponse {
        replace_start_byte: 0,
        replace_end_byte: 0,
        candidates,
        enrichment_pending: false,
    })
}

fn fzf_indexes(candidates: &[Candidate], query: &str, limit: usize) -> Result<Vec<usize>> {
    if candidates.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    // An empty query matches everything in pool order (most recent first).
    if query.trim().is_empty() {
        return Ok((0..candidates.len().min(limit)).collect());
    }
    let mut input = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        write!(input, "{index}\t{}\0", candidate.display)?;
    }

    let child = Command::new("fzf")
        .args([
            "--read0",
            "--print0",
            "--no-multi",
            "--delimiter=\\t",
            "--nth=2..",
            "--tiebreak=index",
            "--filter",
            query,
        ])
        .env_remove("FZF_DEFAULT_OPTS")
        .env_remove("FZF_DEFAULT_OPTS_FILE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(builtin_indexes(candidates, query, limit));
        }
        Err(error) => return Err(error.into()),
    };
    let mut stdin = child.stdin.take().expect("fzf stdin is piped");
    let writer = thread::spawn(move || stdin.write_all(&input));
    let output = child.wait_with_output()?;
    writer.join().expect("fzf input writer panicked")?;
    if output.status.code() == Some(1) {
        return Ok(Vec::new());
    }
    if !output.status.success() {
        bail!("fzf fuzzy filter failed with {}", output.status);
    }

    let mut indexes = Vec::new();
    for record in output.stdout.split(|byte| *byte == 0) {
        if record.is_empty() || indexes.len() >= limit {
            continue;
        }
        let Some(tab) = record.iter().position(|byte| *byte == b'\t') else {
            continue;
        };
        let index = std::str::from_utf8(&record[..tab])?.parse::<usize>()?;
        if index < candidates.len() {
            indexes.push(index);
        }
    }
    Ok(indexes)
}

/// Fallback when `fzf` is not installed: keep pool order and require every
/// whitespace-separated term to occur in the display text, ignoring case.
fn builtin_indexes(candidates: &[Candidate], query: &str, limit: usize) -> Vec<usize> {
    let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| {
            let display = candidate.display.to_lowercase();
            terms.iter().all(|term| display.contains(term.as_str()))
        })
        .map(|(index, _)| index)
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_searches_history_without_a_prefix() {
        if Command::new("fzf").arg("--version").output().is_err() {
            return;
        }
        let store = Store::in_memory().unwrap();
        store
            .record("cargo test --all", "/repo", 0, 100, "test", true)
            .unwrap();
        let completion = fuzzy(
            &store,
            &CommandCatalog::default(),
            "cgt",
            "/repo",
            None,
            FuzzyScope::All,
            &Settings::default(),
        )
        .unwrap();
        assert_eq!(completion.candidates[0].display, "cargo test --all");
    }

    fn history_store() -> Store {
        let store = Store::in_memory().unwrap();
        for (at, command) in [(100, "git status"), (200, "cargo build"), (300, "git log")] {
            store.record(command, "/repo", 0, at, "test", true).unwrap();
        }
        store
    }

    #[test]
    fn history_scope_lists_recent_history_for_an_empty_query() {
        let commands = CommandCatalog::default();
        let completion = fuzzy(
            &history_store(),
            &commands,
            "",
            "/repo",
            None,
            FuzzyScope::History,
            &Settings::default(),
        )
        .unwrap();
        let displays: Vec<_> = completion
            .candidates
            .iter()
            .map(|candidate| candidate.display.as_str())
            .collect();
        assert_eq!(displays, ["git log", "cargo build", "git status"]);
        assert!(
            completion
                .candidates
                .iter()
                .all(|candidate| candidate.source == CandidateSource::History)
        );
    }

    #[test]
    fn history_scope_budget_exceeds_the_completion_budget() {
        let store = Store::in_memory().unwrap();
        for index in 0..100 {
            store
                .record(&format!("echo {index}"), "/repo", 0, index, "test", true)
                .unwrap();
        }
        let completion = fuzzy(
            &store,
            &CommandCatalog::default(),
            "",
            "/repo",
            None,
            FuzzyScope::History,
            &Settings::default(),
        )
        .unwrap();
        assert_eq!(completion.candidates.len(), HISTORY_SEARCH_LIMIT);
        assert_eq!(completion.candidates[0].display, "echo 99");
    }

    #[test]
    fn builtin_matcher_requires_every_term_in_order_of_recency() {
        let pool: Vec<_> = ["git log", "cargo build", "Git Status"]
            .into_iter()
            .map(|command| candidate(command, 1, true))
            .collect();
        assert_eq!(builtin_indexes(&pool, "git", 10), [0, 2]);
        assert_eq!(builtin_indexes(&pool, "st GIT", 10), [2]);
        assert_eq!(builtin_indexes(&pool, "git", 1), [0]);
        assert!(builtin_indexes(&pool, "rustc", 10).is_empty());
    }
}
