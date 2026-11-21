//! Domain layer: tiered completion orchestration.
//!
//! [`complete`] queries providers in strict confidence order — structured
//! help contexts, aliases and installed commands, history, filesystem — and
//! lower tiers only fill capacity the higher tiers left unused. Each provider
//! abstains rather than guess, so an empty response is a valid answer.

mod completion;
mod fuzzy;

use anyhow::{Result, bail};

use crate::catalog::CommandCatalog;
use crate::config::{AcceptMode, Settings};
use crate::persistence::Store;
use crate::transport::protocol::{Alias, Candidate, CompletionResponse};

pub use completion::tiers::filesystem::{filesystem_candidates, merge_filesystem_candidates};
pub use completion::{command_tier, history_tier, structured_tier};
pub use fuzzy::fuzzy;

use completion::tiers::aliases;

const MAX_BUFFER_BYTES: usize = 64 * 1024;

#[allow(clippy::too_many_arguments)]
pub fn complete(
    store: &Store,
    commands: &CommandCatalog,
    buffer: &str,
    cursor_byte: usize,
    cwd: &str,
    requested_limit: Option<usize>,
    aliases: &[Alias],
    settings: &Settings,
) -> Result<CompletionResponse> {
    if buffer.len() > MAX_BUFFER_BYTES {
        bail!("completion buffer exceeds {MAX_BUFFER_BYTES} bytes");
    }
    aliases::validate_aliases(aliases)?;
    if cursor_byte > buffer.len() || !buffer.is_char_boundary(cursor_byte) {
        bail!("cursor is not a valid UTF-8 byte offset");
    }

    // Mid-line replacement requires shell-aware token ranges. Abstain until that
    // parser exists rather than risking corruption of the user's command.
    if cursor_byte != buffer.len() || buffer.is_empty() {
        return Ok(CompletionResponse::empty(cursor_byte));
    }

    let limit = requested_limit
        .unwrap_or(settings.completion.max_candidates)
        .min(settings.completion.max_candidates);

    // Command-first ordering, mirroring ghost-complete: installed commands are
    // offered before any other tier. Structured option, subcommand, and value
    // completions follow, filling capacity commands leave unused. Filesystem
    // matches are merged afterward, and recorded history always sinks to the
    // bottom of the menu.
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut enrichment_pending = false;

    // Tier 1: shell aliases and installed commands — highest confidence,
    // offered first. Aliases precede commands because an alias shadows any
    // executable of the same name when the shell runs the line.
    candidates.extend(command_tier(buffer, aliases, commands, limit));

    // Tier 2: structured completions (options, subcommands, values) for the
    // current command context. They fill capacity commands leave unused. A
    // leading alias is expanded first so `g c` resolves against `git`, while
    // displays and insertions stay relative to the buffer the user typed.
    if candidates.len() < limit {
        let remaining = limit - candidates.len();
        let (help_candidates, pending) = structured_tier(buffer, aliases, commands, remaining);
        enrichment_pending = pending;
        candidates.extend(help_candidates.into_iter().take(remaining));
    }

    // Tier 3: recorded history fills any remaining capacity, always last.
    if candidates.len() < limit {
        let remaining = limit - candidates.len();
        for mut entry in history_tier(store, buffer, cwd, remaining, settings)? {
            if candidates
                .iter()
                .any(|candidate| candidate.display == entry.display)
            {
                continue;
            }
            entry.accept_text = match settings.completion.accept {
                AcceptMode::Segment => completion::segments::next_segment(&entry.insert_text),
                AcceptMode::Full => entry.insert_text.clone(),
            };
            candidates.push(entry);
            if candidates.len() == limit {
                break;
            }
        }
    }

    let response = CompletionResponse {
        replace_start_byte: cursor_byte,
        replace_end_byte: cursor_byte,
        candidates,
        enrichment_pending,
    };

    Ok(response)
}

pub(crate) fn valid_command_prefix(buffer: &str) -> bool {
    !buffer.is_empty()
        && !buffer.starts_with('.')
        && buffer
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'+'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{CommandCatalog, CommandEntry, OptionMatch, SubcommandMatch, ValueMatch};
    use crate::config::Settings;
    use crate::domain::completion::tiers::history::description as history_description;
    use crate::persistence::Store;
    use crate::transport::protocol::{CandidateKind, CandidateSource};

    #[test]
    fn completes_cached_root_command_options() {
        let store = Store::in_memory().unwrap();
        let commands = CommandCatalog::from_options(
            "tool",
            vec![
                OptionMatch {
                    spelling: "--verbose".to_owned(),
                    description: "Show verbose output".to_owned(),
                },
                OptionMatch {
                    spelling: "--version".to_owned(),
                    description: "Print version".to_owned(),
                },
            ],
        );
        let completion = complete(
            &store,
            &commands,
            "tool --ver",
            "tool --ver".len(),
            "/repo",
            None,
            &[],
            &Settings::default(),
        )
        .unwrap();
        assert_eq!(completion.candidates.len(), 2);
        assert_eq!(completion.candidates[0].display, "tool --verbose");
        assert_eq!(completion.candidates[0].kind, CandidateKind::Option);
        assert_eq!(completion.candidates[0].source, CandidateSource::Help);
        assert_eq!(completion.candidates[1].display, "tool --version");
    }

    #[test]
    fn completes_cached_subcommands_and_documented_values() {
        let store = Store::in_memory().unwrap();
        let commands = CommandCatalog::from_structured(
            "tool",
            Vec::new(),
            vec![
                SubcommandMatch {
                    name: "build".to_owned(),
                    description: "Build the project".to_owned(),
                },
                SubcommandMatch {
                    name: "inspect".to_owned(),
                    description: "Inspect project state".to_owned(),
                },
            ],
            vec![(
                "--color".to_owned(),
                vec![
                    ValueMatch {
                        value: "auto".to_owned(),
                        description: String::new(),
                    },
                    ValueMatch {
                        value: "always".to_owned(),
                        description: "Always use color".to_owned(),
                    },
                ],
            )],
        );

        let completion = complete(
            &store,
            &commands,
            "tool bu",
            "tool bu".len(),
            "/repo",
            None,
            &[],
            &Settings::default(),
        )
        .unwrap();
        assert_eq!(completion.candidates.len(), 1);
        assert_eq!(completion.candidates[0].display, "tool build");
        assert_eq!(completion.candidates[0].insert_text, "ild");
        assert_eq!(completion.candidates[0].kind, CandidateKind::Subcommand);

        let completion = complete(
            &store,
            &commands,
            "tool --color=a",
            "tool --color=a".len(),
            "/repo",
            None,
            &[],
            &Settings::default(),
        )
        .unwrap();
        assert_eq!(completion.candidates.len(), 2);
        assert_eq!(completion.candidates[0].display, "tool --color=auto");
        assert_eq!(completion.candidates[0].description, "Value for --color");
        assert_eq!(completion.candidates[0].kind, CandidateKind::Value);
        assert_eq!(completion.candidates[1].display, "tool --color=always");
        assert_eq!(completion.candidates[1].description, "Always use color");

        let completion = complete(
            &store,
            &commands,
            "tool --color auto",
            "tool --color auto".len(),
            "/repo",
            None,
            &[],
            &Settings::default(),
        )
        .unwrap();
        assert_eq!(completion.candidates[0].display, "tool --color auto");
        assert_eq!(completion.candidates[0].insert_text, " ");
    }

    #[test]
    fn describes_history_usage_and_directory() {
        assert_eq!(history_description(1, true), "used here");
        assert_eq!(history_description(3, true), "used 3x, here");
        assert_eq!(history_description(2, false), "used 2x");
    }

    #[test]
    fn completes_history_with_a_single_segment() {
        let store = Store::in_memory().unwrap();
        let mut settings = Settings::default();
        settings.completion.accept = AcceptMode::Segment;
        store
            .record(
                "cd ~/dev/gitrepos/zsuggestion",
                "/repo",
                0,
                100,
                "test",
                true,
            )
            .unwrap();

        let completion = complete(
            &store,
            &CommandCatalog::default(),
            "cd ~/d",
            "cd ~/d".len(),
            "/repo",
            None,
            &[],
            &settings,
        )
        .unwrap();

        assert_eq!(completion.candidates.len(), 1);
        assert_eq!(
            completion.candidates[0].insert_text,
            "ev/gitrepos/zsuggestion"
        );
        assert_eq!(completion.candidates[0].accept_text, "ev/");
        assert_eq!(completion.candidates[0].description, "used here");

        let completion = complete(
            &store,
            &CommandCatalog::default(),
            "cd ~/d",
            "cd ~/d".len(),
            "/repo",
            None,
            &[],
            &Settings::default(),
        )
        .unwrap();
        assert_eq!(
            completion.candidates[0].accept_text,
            "ev/gitrepos/zsuggestion"
        );
    }

    #[test]
    fn abstains_from_mid_line_completion() {
        let store = Store::in_memory().unwrap();
        let completion = complete(
            &store,
            &CommandCatalog::default(),
            "git status",
            3,
            "/repo",
            None,
            &[],
            &Settings::default(),
        )
        .unwrap();
        assert!(completion.candidates.is_empty());
    }

    #[test]
    fn discovers_commands_when_history_abstains() {
        let store = Store::in_memory().unwrap();
        let commands = CommandCatalog::from_entries([CommandEntry {
            name: "atlas".to_owned(),
            description: "CLI tool to manage MongoDB Atlas".to_owned(),
        }]);

        let completion = complete(
            &store,
            &commands,
            "atl",
            3,
            "/repo",
            None,
            &[],
            &Settings::default(),
        )
        .unwrap();

        assert_eq!(completion.candidates[0].display, "atlas");
        assert_eq!(completion.candidates[0].accept_text, "as");
        assert_eq!(completion.candidates[0].kind, CandidateKind::Command);
    }

    #[test]
    fn command_inventory_precedes_history() {
        let store = Store::in_memory().unwrap();
        store
            .record("git status", "/repo", 0, 100, "test", true)
            .unwrap();
        let commands = CommandCatalog::from_entries([CommandEntry {
            name: "git-town".to_owned(),
            description: "Git workflow automation".to_owned(),
        }]);

        let completion = complete(
            &store,
            &commands,
            "git",
            3,
            "/repo",
            None,
            &[],
            &Settings::default(),
        )
        .unwrap();

        assert_eq!(completion.candidates[0].source, CandidateSource::Command);
        assert_eq!(completion.candidates[1].source, CandidateSource::History);
    }

    #[test]
    fn command_inventory_deduplicates_history() {
        let store = Store::in_memory().unwrap();
        store
            .record("atlas", "/repo", 0, 100, "test", true)
            .unwrap();
        let commands = CommandCatalog::from_entries([
            CommandEntry {
                name: "atlas".to_owned(),
                description: "CLI tool to manage MongoDB Atlas".to_owned(),
            },
            CommandEntry {
                name: "atlantis".to_owned(),
                description: "Terraform pull request automation".to_owned(),
            },
        ]);

        let completion = complete(
            &store,
            &commands,
            "atl",
            3,
            "/repo",
            None,
            &[],
            &Settings::default(),
        )
        .unwrap();

        assert_eq!(completion.candidates.len(), 2);
        assert_eq!(completion.candidates[0].display, "atlantis");
        assert_eq!(completion.candidates[0].source, CandidateSource::Command);
        assert_eq!(completion.candidates[1].display, "atlas");
        assert_eq!(completion.candidates[1].source, CandidateSource::Command);
    }
}
