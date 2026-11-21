//! Tiered completion providers.
//!
//! Each tier builds `Candidate` vectors for [`super::super::complete`] to
//! merge in strict confidence order.

pub(super) mod context;
pub(super) mod segments;
pub(super) mod tiers;

use anyhow::Result;

use crate::catalog::CommandCatalog;
use crate::config::{AcceptMode, Settings};
use crate::persistence::Store;
use crate::transport::protocol::{Alias, Candidate, CandidateKind, CandidateSource};

use self::context::{option_context, subcommand_context, value_context};
use self::segments::next_segment;
use self::tiers::{aliases, history};
use super::valid_command_prefix;

/// Tier 1: shell aliases and installed commands.
///
/// Aliases precede commands because an alias shadows any executable of the
/// same name when the shell runs the line.
pub fn command_tier(
    buffer: &str,
    aliases: &[Alias],
    commands: &CommandCatalog,
    limit: usize,
) -> Vec<Candidate> {
    let mut candidates: Vec<Candidate> = Vec::new();
    if !valid_command_prefix(buffer) {
        return candidates;
    }
    candidates.extend(aliases::alias_candidates(buffer, aliases, limit));
    let remaining = limit - candidates.len();
    let command_candidates: Vec<_> = commands
        .matching(buffer, remaining)
        .into_iter()
        .filter(|entry| {
            !candidates
                .iter()
                .any(|candidate| candidate.display == entry.name)
        })
        .map(|entry| {
            let suffix = entry.name.strip_prefix(buffer).unwrap_or_default();
            let insertion = if suffix.is_empty() { " " } else { suffix };
            Candidate {
                display: entry.name.clone(),
                description: entry.description.clone(),
                description_pending: entry.description_pending,
                kind: CandidateKind::Command,
                insert_text: insertion.to_owned(),
                accept_text: insertion.to_owned(),
                source: CandidateSource::Command,
            }
        })
        .collect();
    candidates.extend(command_candidates);
    candidates
}

/// Tier 2: structured completions (options, subcommands, values).
///
/// A leading alias is expanded first so `g c` resolves against `git`, while
/// displays and insertions stay relative to the buffer the user typed.
pub fn structured_tier(
    buffer: &str,
    aliases: &[Alias],
    commands: &CommandCatalog,
    limit: usize,
) -> (Vec<Candidate>, bool) {
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut enrichment_pending = false;

    if candidates.len() < limit {
        let remaining = limit - candidates.len();
        let expanded = aliases::expand_leading_alias(buffer, aliases);
        let context = expanded.as_deref().unwrap_or(buffer);
        let help_candidates = if let Some((command, option, prefix)) = value_context(context) {
            let values = commands.matching_values(command, option, prefix, remaining);
            enrichment_pending = values.pending;
            values
                .entries
                .into_iter()
                .filter_map(|value| {
                    let suffix = value.value.strip_prefix(prefix)?;
                    let insertion = if suffix.is_empty() { " " } else { suffix };
                    Some(Candidate {
                        display: if suffix.is_empty() {
                            buffer.to_owned()
                        } else {
                            format!("{buffer}{suffix}")
                        },
                        description: if value.description.is_empty() {
                            format!("Value for {option}")
                        } else {
                            value.description
                        },
                        description_pending: false,
                        kind: CandidateKind::Value,
                        insert_text: insertion.to_owned(),
                        accept_text: insertion.to_owned(),
                        source: CandidateSource::Help,
                    })
                })
                .collect()
        } else if let Some((command, prefix)) = option_context(context) {
            let options = commands.matching_options(command, prefix, remaining);
            enrichment_pending = options.pending;
            options
                .entries
                .into_iter()
                .filter_map(|option| {
                    let suffix = option.spelling.strip_prefix(prefix)?;
                    if suffix.is_empty() {
                        return None;
                    }
                    Some(Candidate {
                        display: format!("{buffer}{suffix}"),
                        description: option.description,
                        description_pending: false,
                        kind: CandidateKind::Option,
                        insert_text: suffix.to_owned(),
                        accept_text: suffix.to_owned(),
                        source: CandidateSource::Help,
                    })
                })
                .collect()
        } else if let Some((command, prefix)) = subcommand_context(context) {
            let subcommands = commands.matching_subcommands(command, prefix, remaining);
            enrichment_pending = subcommands.pending;
            subcommands
                .entries
                .into_iter()
                .filter_map(|subcommand| {
                    let suffix = subcommand.name.strip_prefix(prefix)?;
                    let insertion = if suffix.is_empty() { " " } else { suffix };
                    Some(Candidate {
                        display: if suffix.is_empty() {
                            buffer.to_owned()
                        } else {
                            format!("{buffer}{suffix}")
                        },
                        description: subcommand.description,
                        description_pending: false,
                        kind: CandidateKind::Subcommand,
                        insert_text: insertion.to_owned(),
                        accept_text: insertion.to_owned(),
                        source: CandidateSource::Help,
                    })
                })
                .collect()
        } else {
            Vec::new()
        };
        candidates.extend(help_candidates.into_iter().take(remaining));
    }

    (candidates, enrichment_pending)
}

/// Tier 3: recorded history fills any remaining capacity, always last.
pub fn history_tier(
    store: &Store,
    buffer: &str,
    cwd: &str,
    limit: usize,
    settings: &Settings,
) -> Result<Vec<Candidate>> {
    let mut candidates = Vec::new();
    for mut entry in
        history::candidates(store, buffer, cwd, limit, settings.history.successful_first)?
    {
        if candidates
            .iter()
            .any(|candidate: &Candidate| candidate.display == entry.display)
        {
            continue;
        }
        entry.accept_text = match settings.completion.accept {
            AcceptMode::Segment => next_segment(&entry.insert_text),
            AcceptMode::Full => entry.insert_text.clone(),
        };
        candidates.push(entry);
        if candidates.len() == limit {
            break;
        }
    }
    Ok(candidates)
}
