//! Tiered candidate providers behind the daemon's completion requests.
//!
//! [`complete`] queries providers in strict confidence order — structured
//! help contexts, aliases and installed commands, history, filesystem — and
//! lower tiers only fill capacity the higher tiers left unused. Each provider
//! abstains rather than guess, so an empty response is a valid answer.

mod alias;
mod context;
mod filesystem;
mod fuzzy;
mod history;

use anyhow::{Result, bail};

use crate::commands::CommandCatalog;
use crate::config::{AcceptMode, Settings};
use crate::protocol::{Alias, Candidate, CandidateKind, CandidateSource, CompletionResponse};
use crate::store::Store;

pub use filesystem::{filesystem_candidates, merge_filesystem_candidates};
pub use fuzzy::fuzzy;

use alias::{alias_candidates, expand_leading_alias, validate_aliases};
use context::{option_context, subcommand_context, value_context};

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
    validate_aliases(aliases)?;
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
    if valid_command_prefix(buffer) {
        candidates.extend(alias_candidates(buffer, aliases, limit));
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
    }

    // Tier 2: structured completions (options, subcommands, values) for the
    // current command context. They fill capacity commands leave unused. A
    // leading alias is expanded first so `g c` resolves against `git`, while
    // displays and insertions stay relative to the buffer the user typed.
    if candidates.len() < limit {
        let remaining = limit - candidates.len();
        let expanded = expand_leading_alias(buffer, aliases);
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

    // Tier 3: recorded history fills any remaining capacity, always last.
    if candidates.len() < limit {
        let remaining = limit - candidates.len();
        for mut entry in history::candidates(
            store,
            buffer,
            cwd,
            remaining,
            settings.history.successful_first,
        )? {
            if candidates
                .iter()
                .any(|candidate| candidate.display == entry.display)
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
    }

    Ok(CompletionResponse {
        replace_start_byte: cursor_byte,
        replace_end_byte: cursor_byte,
        candidates,
        enrichment_pending,
    })
}

fn valid_command_prefix(buffer: &str) -> bool {
    !buffer.is_empty()
        && !buffer.starts_with('.')
        && buffer
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'+'))
}

pub fn next_segment(suffix: &str) -> String {
    let mut saw_non_whitespace = false;
    let mut escaped = false;
    let mut quote = None;
    let mut bracket_depth: usize = 0;
    let mut characters = suffix.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        let end = index + character.len_utf8();
        if escaped {
            escaped = false;
            saw_non_whitespace = true;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            saw_non_whitespace = true;
            continue;
        }
        if matches!(character, '\'' | '"') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            }
            saw_non_whitespace = true;
            continue;
        }
        if quote.is_some() {
            saw_non_whitespace = true;
            continue;
        }
        if character.is_whitespace() {
            if saw_non_whitespace {
                return suffix[..end].to_owned();
            }
            continue;
        }
        saw_non_whitespace = true;
        match character {
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '@' | '=' | ',' if bracket_depth == 0 => return suffix[..end].to_owned(),
            ':' if bracket_depth == 0 => {
                let mut boundary = end;
                while let Some((next_index, next)) = characters.peek().copied() {
                    if !matches!(next, ':' | '/') {
                        break;
                    }
                    characters.next();
                    boundary = next_index + next.len_utf8();
                }
                return suffix[..boundary].to_owned();
            }
            '/' if bracket_depth == 0 => {
                let mut boundary = end;
                while let Some((next_index, '/')) = characters.peek().copied() {
                    characters.next();
                    boundary = next_index + 1;
                }
                return suffix[..boundary].to_owned();
            }
            _ => {}
        }
    }
    suffix.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{CommandCatalog, CommandEntry, OptionMatch, SubcommandMatch, ValueMatch};
    use crate::config::Settings;
    use crate::engine::history::{description as history_description, sanitize_display};
    use crate::store::Store;

    #[test]
    fn accepts_one_path_segment() {
        assert_eq!(next_segment("ev/gitrepos/zsuggestion"), "ev/");
    }

    #[test]
    fn accepts_one_shell_word() {
        assert_eq!(next_segment(" checkout feature/topic"), " checkout ");
    }

    #[test]
    fn accepts_remaining_text_without_boundary() {
        assert_eq!(next_segment("status"), "status");
    }

    #[test]
    fn accepts_ssh_destinations_in_semantic_segments() {
        assert_eq!(next_segment("lice@example.com"), "lice@");
        assert_eq!(next_segment("example.com:/srv/app/file"), "example.com:/");
        assert_eq!(next_segment("srv/app/file"), "srv/");
    }

    #[test]
    fn accepts_common_structured_values_in_semantic_segments() {
        assert_eq!(next_segment("output=value"), "output=");
        assert_eq!(next_segment("https://example.com/path"), "https://");
        assert_eq!(next_segment("host::module/path"), "host::");
        assert_eq!(next_segment("one,two"), "one,");
    }

    #[test]
    fn preserves_quoted_escaped_and_ipv6_separators() {
        assert_eq!(next_segment("'user@host'/path"), "'user@host'/");
        assert_eq!(next_segment("user\\@host/path"), "user\\@host/");
        assert_eq!(next_segment("user@[2001:db8::1]:/srv/app"), "user@");
        assert_eq!(next_segment("[2001:db8::1]:/srv/app"), "[2001:db8::1]:/");
    }

    #[test]
    fn escapes_control_characters_in_display_text() {
        assert_eq!(sanitize_display("echo\t\u{1b}"), "echo\\t\\u{1b}");
    }

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
