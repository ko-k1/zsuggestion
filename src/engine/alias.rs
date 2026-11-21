//! Shell aliases sent by the frontend: validation, `Cmd` candidates for
//! alias names, and expansion of the leading alias chain so structured
//! contexts resolve through `alias g=git`.

use std::collections::HashSet;

use anyhow::{Result, bail};

use super::valid_command_prefix;
use crate::engine::history::sanitize_display;
use crate::protocol::{Alias, Candidate, CandidateKind, CandidateSource};

const MAX_ALIASES: usize = 64;

const MAX_ALIAS_NAME_BYTES: usize = 256;

const MAX_ALIAS_VALUE_BYTES: usize = 4096;

const MAX_ALIAS_DEPTH: usize = 8;

const MAX_ALIAS_DESCRIPTION_CHARS: usize = 72;

pub(super) fn validate_aliases(aliases: &[Alias]) -> Result<()> {
    if aliases.len() > MAX_ALIASES {
        bail!("completion request carries more than {MAX_ALIASES} aliases");
    }
    if aliases.iter().any(|alias| {
        alias.name.len() > MAX_ALIAS_NAME_BYTES || alias.value.len() > MAX_ALIAS_VALUE_BYTES
    }) {
        bail!("alias definition exceeds the allowed size");
    }
    Ok(())
}

pub(super) fn alias_candidates(prefix: &str, aliases: &[Alias], limit: usize) -> Vec<Candidate> {
    let mut matches: Vec<&Alias> = aliases
        .iter()
        .filter(|alias| alias.name.starts_with(prefix) && valid_command_prefix(&alias.name))
        .collect();
    matches.sort_by(|left, right| left.name.cmp(&right.name));
    matches.dedup_by(|left, right| left.name == right.name);
    matches
        .into_iter()
        .take(limit)
        .map(|alias| {
            let suffix = &alias.name[prefix.len()..];
            let insertion = if suffix.is_empty() { " " } else { suffix };
            Candidate {
                display: alias.name.clone(),
                description: alias_description(&alias.value),
                description_pending: false,
                kind: CandidateKind::Command,
                insert_text: insertion.to_owned(),
                accept_text: insertion.to_owned(),
                source: CandidateSource::Command,
            }
        })
        .collect()
}

fn alias_description(value: &str) -> String {
    let value = sanitize_display(value.trim());
    let mut description = String::from("alias for ");
    if value.chars().count() > MAX_ALIAS_DESCRIPTION_CHARS {
        description.extend(value.chars().take(MAX_ALIAS_DESCRIPTION_CHARS - 1));
        description.push('…');
    } else {
        description.push_str(&value);
    }
    description
}

/// Expands the command word of `buffer` the way Zsh expands a leading alias,
/// repeating while the expansion's own first word is another alias. Returns
/// `None` while the command word is still being typed or names no alias.
pub(super) fn expand_leading_alias(buffer: &str, aliases: &[Alias]) -> Option<String> {
    let mut expanded = buffer.to_owned();
    let mut seen = HashSet::new();
    let mut changed = false;
    for _ in 0..MAX_ALIAS_DEPTH {
        let Some(word_end) = expanded.find(char::is_whitespace) else {
            break;
        };
        let word = &expanded[..word_end];
        if word.is_empty() || !seen.insert(word.to_owned()) {
            break;
        }
        let Some(alias) = aliases.iter().find(|alias| alias.name == word) else {
            break;
        };
        expanded = format!("{}{}", alias.value.trim_start(), &expanded[word_end..]);
        changed = true;
    }
    changed.then_some(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{CommandCatalog, CommandEntry, OptionMatch, SubcommandMatch};
    use crate::config::Settings;
    use crate::engine::complete;
    use crate::store::Store;

    fn alias(name: &str, value: &str) -> Alias {
        Alias {
            name: name.to_owned(),
            value: value.to_owned(),
        }
    }

    #[test]
    fn offers_aliases_before_shadowed_commands() {
        let store = Store::in_memory().unwrap();
        let commands = CommandCatalog::from_entries([
            CommandEntry {
                name: "gcc".to_owned(),
                description: "GNU C compiler".to_owned(),
            },
            CommandEntry {
                name: "gst".to_owned(),
                description: "GStreamer tool".to_owned(),
            },
        ]);
        let aliases = [alias("gst", "git status"), alias("ga", "git add")];

        let completion = complete(
            &store,
            &commands,
            "g",
            1,
            "/repo",
            None,
            &aliases,
            &Settings::default(),
        )
        .unwrap();

        let displays: Vec<_> = completion
            .candidates
            .iter()
            .map(|candidate| candidate.display.as_str())
            .collect();
        assert_eq!(displays, ["ga", "gst", "gcc"]);
        assert_eq!(completion.candidates[1].description, "alias for git status");
        assert_eq!(completion.candidates[1].insert_text, "st");
        assert_eq!(completion.candidates[1].kind, CandidateKind::Command);
        assert!(!completion.candidates[1].description_pending);
    }

    #[test]
    fn exact_alias_inserts_a_trailing_space() {
        let store = Store::in_memory().unwrap();
        let commands = CommandCatalog::from_entries([]);
        let completion = complete(
            &store,
            &commands,
            "ll",
            2,
            "/repo",
            None,
            &[alias("ll", "ls -la\nrm -rf /")],
            &Settings::default(),
        )
        .unwrap();
        assert_eq!(completion.candidates[0].insert_text, " ");
        assert_eq!(
            completion.candidates[0].description,
            "alias for ls -la\\nrm -rf /"
        );
    }

    #[test]
    fn expands_leading_alias_chains_for_structured_contexts() {
        let aliases = [alias("g", "git"), alias("gg", "g --git-dir=x")];
        assert_eq!(expand_leading_alias("g", &aliases), None);
        assert_eq!(
            expand_leading_alias("g c", &aliases).as_deref(),
            Some("git c")
        );
        assert_eq!(
            expand_leading_alias("gg -", &aliases).as_deref(),
            Some("git --git-dir=x -")
        );
        assert_eq!(expand_leading_alias("git c", &aliases), None);
        assert_eq!(expand_leading_alias(" g c", &aliases), None);

        let cyclic = [alias("ls", "ls --color"), alias("a", "b"), alias("b", "a")];
        assert_eq!(
            expand_leading_alias("ls -", &cyclic).as_deref(),
            Some("ls --color -")
        );
        assert_eq!(expand_leading_alias("a x", &cyclic).as_deref(), Some("a x"));
    }

    #[test]
    fn completes_subcommands_and_options_through_an_alias() {
        let store = Store::in_memory().unwrap();
        let commands = CommandCatalog::from_structured(
            "tool",
            vec![OptionMatch {
                spelling: "--verbose".to_owned(),
                description: "Show verbose output".to_owned(),
            }],
            vec![SubcommandMatch {
                name: "build".to_owned(),
                description: "Build the project".to_owned(),
            }],
            Vec::new(),
        );
        let aliases = [alias("t", "tool"), alias("tq", "t --verbose")];

        let completion = complete(
            &store,
            &commands,
            "t bu",
            4,
            "/repo",
            None,
            &aliases,
            &Settings::default(),
        )
        .unwrap();
        assert_eq!(completion.candidates[0].display, "t build");
        assert_eq!(completion.candidates[0].insert_text, "ild");
        assert_eq!(completion.candidates[0].kind, CandidateKind::Subcommand);

        let completion = complete(
            &store,
            &commands,
            "tq --verb",
            9,
            "/repo",
            None,
            &aliases,
            &Settings::default(),
        )
        .unwrap();
        assert_eq!(completion.candidates[0].display, "tq --verbose");
        assert_eq!(completion.candidates[0].insert_text, "ose");

        // Without the alias the command word is unknown and the engine abstains.
        let completion = complete(
            &store,
            &commands,
            "t bu",
            4,
            "/repo",
            None,
            &[],
            &Settings::default(),
        )
        .unwrap();
        assert!(completion.candidates.is_empty());
    }

    #[test]
    fn rejects_oversized_alias_payloads() {
        let store = Store::in_memory().unwrap();
        let commands = CommandCatalog::from_entries([]);
        let many: Vec<_> = (0..=MAX_ALIASES)
            .map(|index| alias(&format!("a{index}"), "x"))
            .collect();
        assert!(
            complete(
                &store,
                &commands,
                "a",
                1,
                "/",
                None,
                &many,
                &Settings::default()
            )
            .is_err()
        );
        let large = [alias("a", &"x".repeat(MAX_ALIAS_VALUE_BYTES + 1))];
        assert!(
            complete(
                &store,
                &commands,
                "a",
                1,
                "/",
                None,
                &large,
                &Settings::default()
            )
            .is_err()
        );
    }
}
