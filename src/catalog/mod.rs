//! Installed-command inventory with lazy, cached enrichment.
//!
//! [`CommandCatalog`] answers prefix queries for command names, options,
//! subcommands, and option values without blocking: unknown commands are
//! queued for background workers that probe `man`/`--help` (`probe`), parse
//! the output (`parse`), and persist results in a fingerprinted JSON cache
//! (`cache`).
//!
//! Some items here are referenced by sibling modules via `super::super::`
//! paths; `dead_code` is allowed because cross-module usage isn't always
//! visible to the lint.
#![allow(dead_code)]

mod enrichment;
mod inventory;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

const CACHE_VERSION: u32 = 4;
const CACHE_MAX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_OPTION_COUNT: usize = 256;
const MAX_OPTION_SPELLING_BYTES: usize = 128;
const MAX_SUBCOMMAND_COUNT: usize = 256;
const MAX_SUBCOMMAND_NAME_BYTES: usize = 128;
const MAX_VALUE_COUNT: usize = 128;
const MAX_VALUE_BYTES: usize = 128;
const SUCCESS_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const MISS_TTL: Duration = Duration::from_secs(24 * 60 * 60);

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandEntry {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandMatch {
    pub name: String,
    pub description: String,
    pub description_pending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptionMatch {
    pub spelling: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionMatches {
    pub entries: Vec<OptionMatch>,
    pub pending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubcommandMatch {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubcommandMatches {
    pub entries: Vec<SubcommandMatch>,
    pub pending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueMatch {
    pub value: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueMatches {
    pub entries: Vec<ValueMatch>,
    pub pending: bool,
}

#[derive(Debug)]
pub struct CommandCatalog {
    entries: Vec<CommandEntry>,
    jobs: HashMap<String, DescriptionJob>,
    state: Arc<Mutex<EnrichmentState>>,
    queue: Option<SyncSender<DescriptionJob>>,
}

#[derive(Debug, Default)]
struct EnrichmentState {
    descriptions: HashMap<String, String>,
    options: HashMap<String, Vec<OptionMatch>>,
    subcommands: HashMap<String, Vec<SubcommandMatch>>,
    option_values: HashMap<String, Vec<OptionValues>>,
    settled: HashSet<String>,
    pending: HashSet<String>,
}

#[derive(Debug, Clone)]
struct DescriptionJob {
    name: String,
    path: PathBuf,
    fingerprint: ExecutableFingerprint,
    authored_description: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ExecutableFingerprint {
    path: PathBuf,
    size: u64,
    device: u64,
    inode: u64,
    mode: u32,
    modified_secs: i64,
    modified_nanos: i64,
    changed_secs: i64,
    changed_nanos: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedDescription {
    fingerprint: ExecutableFingerprint,
    checked_at_secs: u64,
    description: Option<String>,
    options: Vec<OptionMatch>,
    subcommands: Vec<SubcommandMatch>,
    option_values: Vec<OptionValues>,
}

#[derive(Debug, Default)]
struct Enrichment {
    description: Option<String>,
    options: Vec<OptionMatch>,
    subcommands: Vec<SubcommandMatch>,
    option_values: Vec<OptionValues>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct OptionValues {
    option: String,
    values: Vec<ValueMatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DescriptionCache {
    version: u32,
    entries: BTreeMap<String, CachedDescription>,
}

impl Default for DescriptionCache {
    fn default() -> Self {
        Self {
            version: CACHE_VERSION,
            entries: BTreeMap::new(),
        }
    }
}

impl Default for CommandCatalog {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            jobs: HashMap::new(),
            state: Arc::new(Mutex::new(EnrichmentState::default())),
            queue: None,
        }
    }
}

impl CommandCatalog {
    pub fn matching(&self, prefix: &str, limit: usize) -> Vec<CommandMatch> {
        let mut state = self.state.lock().expect("command state lock poisoned");
        self.entries
            .iter()
            .filter(|entry| entry.name.starts_with(prefix))
            .take(limit)
            .map(|entry| {
                let job = self.jobs.get(&entry.name);
                let description = if job.is_some_and(|job| job.authored_description) {
                    entry.description.clone()
                } else {
                    state
                        .descriptions
                        .get(&entry.name)
                        .cloned()
                        .unwrap_or_else(|| entry.description.clone())
                };
                let metadata_pending = enqueue_if_unsettled(&mut state, job, self.queue.as_ref());
                let description_pending =
                    metadata_pending && !job.is_some_and(|job| job.authored_description);

                CommandMatch {
                    name: entry.name.clone(),
                    description,
                    description_pending,
                }
            })
            .collect()
    }

    pub fn matching_options(&self, command: &str, prefix: &str, limit: usize) -> OptionMatches {
        let Some(job) = self.jobs.get(command) else {
            return OptionMatches {
                entries: Vec::new(),
                pending: false,
            };
        };
        let mut state = self.state.lock().expect("command state lock poisoned");
        let pending = enqueue_if_unsettled(&mut state, Some(job), self.queue.as_ref());
        let entries = state
            .options
            .get(command)
            .into_iter()
            .flatten()
            .filter(|option| option.spelling.starts_with(prefix))
            .take(limit)
            .cloned()
            .collect();
        OptionMatches { entries, pending }
    }

    pub fn matching_subcommands(
        &self,
        command: &str,
        prefix: &str,
        limit: usize,
    ) -> SubcommandMatches {
        let Some(job) = self.jobs.get(command) else {
            return SubcommandMatches {
                entries: Vec::new(),
                pending: false,
            };
        };
        let mut state = self.state.lock().expect("command state lock poisoned");
        let pending = enqueue_if_unsettled(&mut state, Some(job), self.queue.as_ref());
        let entries = state
            .subcommands
            .get(command)
            .into_iter()
            .flatten()
            .filter(|subcommand| subcommand.name.starts_with(prefix))
            .take(limit)
            .cloned()
            .collect();
        SubcommandMatches { entries, pending }
    }

    pub fn matching_values(
        &self,
        command: &str,
        option: &str,
        prefix: &str,
        limit: usize,
    ) -> ValueMatches {
        let Some(job) = self.jobs.get(command) else {
            return ValueMatches {
                entries: Vec::new(),
                pending: false,
            };
        };
        let mut state = self.state.lock().expect("command state lock poisoned");
        let pending = enqueue_if_unsettled(&mut state, Some(job), self.queue.as_ref());
        let entries = state
            .option_values
            .get(command)
            .into_iter()
            .flatten()
            .find(|values| values.option == option)
            .into_iter()
            .flat_map(|values| &values.values)
            .filter(|value| value.value.starts_with(prefix))
            .take(limit)
            .cloned()
            .collect();
        ValueMatches { entries, pending }
    }

    pub fn inventory(&self) -> Vec<CommandMatch> {
        let state = self.state.lock().expect("command state lock poisoned");
        self.entries
            .iter()
            .map(|entry| CommandMatch {
                name: entry.name.clone(),
                description: if self
                    .jobs
                    .get(&entry.name)
                    .is_some_and(|job| job.authored_description)
                {
                    entry.description.clone()
                } else {
                    state
                        .descriptions
                        .get(&entry.name)
                        .cloned()
                        .unwrap_or_else(|| entry.description.clone())
                },
                description_pending: false,
            })
            .collect()
    }

    #[cfg(test)]
    pub fn from_entries(entries: impl IntoIterator<Item = CommandEntry>) -> Self {
        let mut entries: Vec<_> = entries.into_iter().collect();
        entries.sort_unstable_by(|left, right| left.name.cmp(&right.name));
        Self {
            entries,
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub fn from_options(command: &str, options: Vec<OptionMatch>) -> Self {
        Self::from_structured(command, options, Vec::new(), Vec::new())
    }

    #[cfg(test)]
    pub fn from_structured(
        command: &str,
        options: Vec<OptionMatch>,
        subcommands: Vec<SubcommandMatch>,
        option_values: Vec<(String, Vec<ValueMatch>)>,
    ) -> Self {
        let entry = CommandEntry {
            name: command.to_owned(),
            description: "Test command".to_owned(),
        };
        let job = DescriptionJob {
            name: command.to_owned(),
            path: PathBuf::from(command),
            fingerprint: ExecutableFingerprint {
                path: PathBuf::from(command),
                size: 0,
                device: 0,
                inode: 0,
                mode: 0,
                modified_secs: 0,
                modified_nanos: 0,
                changed_secs: 0,
                changed_nanos: 0,
            },
            authored_description: true,
        };
        let mut state = EnrichmentState::default();
        state.options.insert(command.to_owned(), options);
        state.subcommands.insert(command.to_owned(), subcommands);
        state.option_values.insert(
            command.to_owned(),
            option_values
                .into_iter()
                .map(|(option, values)| OptionValues { option, values })
                .collect(),
        );
        state.settled.insert(command.to_owned());
        Self {
            entries: vec![entry],
            jobs: HashMap::from([(command.to_owned(), job)]),
            state: Arc::new(Mutex::new(state)),
            queue: None,
        }
    }
}

fn enqueue_if_unsettled(
    state: &mut EnrichmentState,
    job: Option<&DescriptionJob>,
    queue: Option<&SyncSender<DescriptionJob>>,
) -> bool {
    let (Some(job), Some(queue)) = (job, queue) else {
        return false;
    };
    if state.settled.contains(&job.name) {
        return false;
    }
    if state.pending.insert(job.name.clone()) {
        match queue.try_send(job.clone()) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                state.pending.remove(&job.name);
            }
            Err(TrySendError::Disconnected(_)) => {
                state.pending.remove(&job.name);
                state.settled.insert(job.name.clone());
                return false;
            }
        }
    }
    true
}

fn valid_subcommand_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_SUBCOMMAND_NAME_BYTES
        && name.is_ascii()
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'))
}

fn valid_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_VALUE_BYTES
        && value.is_ascii()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+' | b'/')
        })
}

#[cfg(test)]
mod tests {
    use super::enrichment::cache::{load_cache, save_cache};
    use super::inventory::valid_name;
    use super::*;
    use std::sync::mpsc::sync_channel;
    use std::time::Instant;
    use tempfile::tempdir;

    #[test]
    fn matches_sorted_prefixes() {
        let catalog = CommandCatalog::from_entries([
            CommandEntry {
                name: "atlas".to_owned(),
                description: "MongoDB Atlas".to_owned(),
            },
            CommandEntry {
                name: "arch".to_owned(),
                description: "Architecture".to_owned(),
            },
        ]);

        let names: Vec<_> = catalog
            .matching("a", 10)
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(names, ["arch", "atlas"]);
    }

    #[test]
    fn rejects_shell_metacharacters_in_names() {
        assert!(valid_name("aws-vault"));
        assert!(!valid_name("bad command"));
        assert!(!valid_name("bad;command"));
    }

    #[test]
    fn description_cache_round_trips() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("descriptions.json");
        let mut cache = DescriptionCache::default();
        cache.entries.insert(
            "tool".to_owned(),
            CachedDescription {
                fingerprint: ExecutableFingerprint {
                    path: PathBuf::from("/usr/bin/tool"),
                    size: 42,
                    device: 1,
                    inode: 2,
                    mode: 0o100755,
                    modified_secs: 3,
                    modified_nanos: 4,
                    changed_secs: 5,
                    changed_nanos: 6,
                },
                checked_at_secs: 7,
                description: Some("Inspect a tool".to_owned()),
                options: vec![OptionMatch {
                    spelling: "--verbose".to_owned(),
                    description: "Show more detail".to_owned(),
                }],
                subcommands: vec![SubcommandMatch {
                    name: "inspect".to_owned(),
                    description: "Inspect a project".to_owned(),
                }],
                option_values: vec![OptionValues {
                    option: "--color".to_owned(),
                    values: vec![ValueMatch {
                        value: "always".to_owned(),
                        description: String::new(),
                    }],
                }],
            },
        );

        save_cache(&path, &cache).unwrap();
        let loaded = load_cache(&path);
        assert_eq!(
            loaded.entries["tool"].description.as_deref(),
            Some("Inspect a tool")
        );
        assert_eq!(loaded.entries["tool"].fingerprint.size, 42);
        assert_eq!(
            loaded.entries["tool"].options,
            [OptionMatch {
                spelling: "--verbose".to_owned(),
                description: "Show more detail".to_owned(),
            }]
        );
        assert_eq!(loaded.entries["tool"].subcommands[0].name, "inspect");
        assert_eq!(
            loaded.entries["tool"].option_values[0].values[0].value,
            "always"
        );
    }

    #[test]
    fn authored_descriptions_remain_preferred_during_enrichment() {
        let job = test_job("cargo", true);
        let mut state = EnrichmentState::default();
        state
            .descriptions
            .insert("cargo".to_owned(), "Parsed description".to_owned());
        let catalog = CommandCatalog {
            entries: vec![CommandEntry {
                name: "cargo".to_owned(),
                description: "Rust package manager and build tool".to_owned(),
            }],
            jobs: HashMap::from([("cargo".to_owned(), job)]),
            state: Arc::new(Mutex::new(state)),
            queue: None,
        };

        assert_eq!(
            catalog.matching("cargo", 1)[0].description,
            "Rust package manager and build tool"
        );
    }

    #[test]
    fn matches_cached_options_by_prefix_for_an_exact_command() {
        let mut state = EnrichmentState::default();
        state.settled.insert("tool".to_owned());
        state.options.insert(
            "tool".to_owned(),
            vec![
                OptionMatch {
                    spelling: "--all".to_owned(),
                    description: "Include all".to_owned(),
                },
                OptionMatch {
                    spelling: "--color".to_owned(),
                    description: "Control color".to_owned(),
                },
                OptionMatch {
                    spelling: "-v".to_owned(),
                    description: "Verbose".to_owned(),
                },
            ],
        );
        let catalog = CommandCatalog {
            jobs: HashMap::from([("tool".to_owned(), test_job("tool", false))]),
            state: Arc::new(Mutex::new(state)),
            ..CommandCatalog::default()
        };

        let matches = catalog.matching_options("tool", "--", 1);
        assert_eq!(matches.entries[0].spelling, "--all");
        assert!(!matches.pending);
        assert!(
            catalog
                .matching_options("unknown", "-", 10)
                .entries
                .is_empty()
        );
        assert!(!catalog.matching_options("unknown", "-", 10).pending);
    }

    #[test]
    fn unsettled_options_report_pending_without_blocking_on_a_full_queue() {
        let (sender, _receiver) = sync_channel(1);
        sender.try_send(test_job("queued", false)).unwrap();
        let catalog = CommandCatalog {
            jobs: HashMap::from([("tool".to_owned(), test_job("tool", false))]),
            queue: Some(sender),
            ..CommandCatalog::default()
        };
        let started = Instant::now();

        let matches = catalog.matching_options("tool", "--", 10);

        assert!(matches.entries.is_empty());
        assert!(matches.pending);
        assert!(started.elapsed() < Duration::from_millis(100));
    }

    fn test_job(name: &str, authored_description: bool) -> DescriptionJob {
        DescriptionJob {
            name: name.to_owned(),
            path: PathBuf::from(format!("/usr/bin/{name}")),
            fingerprint: ExecutableFingerprint {
                path: PathBuf::from(format!("/usr/bin/{name}")),
                size: 1,
                device: 1,
                inode: 1,
                mode: 0o100755,
                modified_secs: 1,
                modified_nanos: 1,
                changed_secs: 1,
                changed_nanos: 1,
            },
            authored_description,
        }
    }
}
