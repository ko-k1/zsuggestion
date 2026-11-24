//! Domain layer: tiered completion orchestration.
//!
//! [`complete`] queries providers in strict confidence order — aliases and
//! installed commands, structured help contexts, filesystem, history — and
//! lists their results in that order. Each tier has its own candidate budget,
//! so a full higher tier never hides every lower-tier result. Each provider
//! abstains rather than guess, so an empty response is a valid answer.

mod completion;
mod fuzzy;

use anyhow::{Result, bail};

use crate::catalog::CommandCatalog;
use crate::config::Settings;
use crate::persistence::Store;
use crate::transport::protocol::{Alias, CandidateKind, CandidateSource, CompletionResponse};

pub use completion::{command_tier, history_tier, structured_tier};
pub use fuzzy::fuzzy;

use completion::tiers::{aliases, filesystem, paths};

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

    // The budget applies to each tier, so the merged response may contain up
    // to four times this many candidates.
    let limit = requested_limit
        .unwrap_or(settings.completion.max_candidates)
        .min(settings.completion.max_candidates);

    // Command-first ordering, mirroring ghost-complete: installed commands are
    // offered first, then structured option, subcommand, and value
    // completions, then filesystem matches, and recorded history always sinks
    // to the bottom of the menu. Each tier gets its own `limit` budget so a
    // full higher tier never hides every directory, file, or history entry;
    // the frontend menu scrolls.
    let mut response = CompletionResponse::empty(cursor_byte);

    // Tier 1: shell aliases and installed commands — highest confidence.
    // Aliases precede commands because an alias shadows any executable of the
    // same name when the shell runs the line.
    let commands_found = command_tier(buffer, aliases, commands, limit);

    // Tier 2: structured completions (options, subcommands, values) for the
    // current command context. A leading alias is expanded first so `g c`
    // resolves against `git`, while displays and insertions stay relative to
    // the buffer the user typed.
    let (structured, pending) = structured_tier(buffer, aliases, commands, limit);
    response.enrichment_pending = pending;

    // Tier 3: bounded filesystem matches.
    let mut paths = filesystem::filesystem_candidates(buffer, cursor_byte, cwd, aliases)?;
    paths.truncate(limit);

    // Tier 4: recorded history, always last.
    let history = history_tier(store, buffer, cwd, limit, settings)?;

    // Filesystem directories are displayed with a trailing slash while a
    // recorded `cd src` keeps the form the user typed. For directory-only
    // commands `dir/` and `dir` are interchangeable, so drop the history row
    // when the filesystem tier already offers the same directory. Other path
    // commands (such as `git checkout`) can name branches or files, so a
    // trailing-slash directory row and a bare path row are kept distinct.
    let expanded = aliases::expand_leading_alias(buffer, aliases);
    let directories_only = paths::classification_command(expanded.as_deref().unwrap_or(buffer))
        .is_some_and(paths::takes_directories_only);
    let directory_displays: std::collections::HashSet<String> = if directories_only {
        paths
            .iter()
            .filter(|candidate| candidate.kind == CandidateKind::Directory)
            .map(|candidate| candidate.display.trim_end_matches('/').to_owned())
            .collect()
    } else {
        std::collections::HashSet::new()
    };

    let mut seen = std::collections::HashSet::new();
    for candidate in commands_found
        .into_iter()
        .chain(structured.into_iter().take(limit))
        .chain(paths)
        .chain(history)
    {
        if !seen.insert(candidate.display.clone()) {
            continue;
        }
        if candidate.source == CandidateSource::History
            && directories_only
            && directory_displays.contains(candidate.display.trim_end_matches('/'))
        {
            continue;
        }
        response.candidates.push(candidate);
    }

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
    use crate::config::{AcceptMode, Settings};
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
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("dev/gitrepos/zsuggestion")).unwrap();
        let cwd = directory.path().to_str().unwrap();
        store
            .record("cd dev/gitrepos/zsuggestion", cwd, 0, 100, "test", true)
            .unwrap();
        let history = |settings: &Settings| {
            complete(
                &store,
                &CommandCatalog::default(),
                "cd d",
                "cd d".len(),
                cwd,
                None,
                &[],
                settings,
            )
            .unwrap()
            .candidates
            .into_iter()
            .filter(|candidate| candidate.source == CandidateSource::History)
            .collect::<Vec<_>>()
        };

        let candidates = history(&settings);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].insert_text, "ev/gitrepos/zsuggestion");
        assert_eq!(candidates[0].accept_text, "ev/");
        assert_eq!(candidates[0].description, "used here");

        let candidates = history(&Settings::default());
        assert_eq!(candidates[0].accept_text, "ev/gitrepos/zsuggestion");
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
    fn filesystem_directory_replaces_duplicate_history_without_slash() {
        let store = Store::in_memory().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let cwd = directory.path().to_str().unwrap();
        std::fs::create_dir(directory.path().join("src")).unwrap();
        std::fs::write(directory.path().join("src/main.rs"), "").unwrap();
        store.record("cd src", cwd, 0, 100, "test", true).unwrap();

        let completion = complete(
            &store,
            &CommandCatalog::default(),
            "cd s",
            "cd s".len(),
            cwd,
            None,
            &[],
            &Settings::default(),
        )
        .unwrap();
        let displays: Vec<_> = completion
            .candidates
            .iter()
            .map(|candidate| candidate.display.as_str())
            .collect();
        assert_eq!(displays, ["cd src/"]);
    }

    #[test]
    fn history_directory_is_kept_for_non_path_commands() {
        let store = Store::in_memory().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let cwd = directory.path().to_str().unwrap();
        std::fs::create_dir(directory.path().join("src")).unwrap();
        store
            .record("git checkout src", cwd, 0, 100, "test", true)
            .unwrap();

        let completion = complete(
            &store,
            &CommandCatalog::default(),
            "git checkout s",
            "git checkout s".len(),
            cwd,
            None,
            &[],
            &Settings::default(),
        )
        .unwrap();
        let displays: Vec<_> = completion
            .candidates
            .iter()
            .map(|candidate| candidate.display.as_str())
            .collect();
        assert!(displays.contains(&"git checkout src/"));
        assert!(
            displays.contains(&"git checkout src"),
            "a branch-like history entry was treated as a duplicate path: {displays:?}"
        );
    }

    #[test]
    fn completes_filesystem_paths_after_structured_candidates() {
        let store = Store::in_memory().unwrap();
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("alpha-file"), "file").unwrap();
        let buffer = "tool ";

        let completion = complete(
            &store,
            &CommandCatalog::default(),
            buffer,
            buffer.len(),
            directory.path().to_str().unwrap(),
            None,
            &[],
            &Settings::default(),
        )
        .unwrap();

        assert!(completion.candidates.iter().any(|candidate| {
            candidate.display == "tool alpha-file" && candidate.kind == CandidateKind::File
        }));
    }

    #[test]
    fn full_structured_tier_does_not_hide_files_or_history() {
        let store = Store::in_memory().unwrap();
        store
            .record("tool sub-0 --flag", "/elsewhere", 0, 100, "test", true)
            .unwrap();
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("sub-file"), "file").unwrap();
        let subcommands = (0..20)
            .map(|index| SubcommandMatch {
                name: format!("sub-{index}"),
                description: String::new(),
            })
            .collect();
        let commands = CommandCatalog::from_structured("tool", Vec::new(), subcommands, Vec::new());

        let completion = complete(
            &store,
            &commands,
            "tool sub",
            "tool sub".len(),
            directory.path().to_str().unwrap(),
            None,
            &[],
            &Settings::default(),
        )
        .unwrap();

        let kinds: Vec<_> = completion.candidates.iter().map(|c| c.kind).collect();
        let limit = Settings::default().completion.max_candidates;
        assert_eq!(
            kinds
                .iter()
                .filter(|kind| **kind == CandidateKind::Subcommand)
                .count(),
            limit
        );
        assert!(
            completion
                .candidates
                .iter()
                .any(|c| c.display == "tool sub-file")
        );
        assert_eq!(
            completion.candidates.last().unwrap().display,
            "tool sub-0 --flag"
        );
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
