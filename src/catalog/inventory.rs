//! Command inventory discovery.
//!
//! Scans `PATH`, loads shell builtins, and seeds the enrichment catalog with
//! fingerprinted jobs for background help/man probing.

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;

use super::enrichment::cache::{
    cache_is_fresh, fingerprint, fingerprint_matches, load_cache, save_cache,
};
use super::enrichment::known;
use super::enrichment::probe::discover_enrichment;
use super::{CommandCatalog, CommandEntry, DescriptionJob, EnrichmentState};
use crate::clock::now_secs;

const QUEUE_CAPACITY: usize = 64;
const WORKER_COUNT: usize = 2;

impl CommandCatalog {
    pub fn discover(cache_file: PathBuf) -> Self {
        let cache = load_cache(&cache_file);
        let now = now_secs();
        let mut seen = HashSet::new();
        let mut entries = Vec::new();
        let mut jobs = HashMap::new();
        let mut state = EnrichmentState::default();

        for (name, description) in known::SHELL_BUILTINS {
            seen.insert((*name).to_owned());
            entries.push(CommandEntry {
                name: (*name).to_owned(),
                description: (*description).to_owned(),
            });
        }

        if let Some(path) = env::var_os("PATH") {
            for directory in env::split_paths(&path) {
                let Ok(children) = fs::read_dir(&directory) else {
                    continue;
                };
                for child in children.flatten() {
                    let Some(name) = child.file_name().to_str().map(str::to_owned) else {
                        continue;
                    };
                    if !valid_name(&name) || jobs.contains_key(&name) {
                        continue;
                    }
                    let path = child.path();
                    let Ok(metadata) = fs::metadata(&path) else {
                        continue;
                    };
                    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
                        continue;
                    }
                    let authored = known::description(&name);
                    let authored_description = !seen.insert(name.clone()) || authored.is_some();
                    if !authored_description || authored.is_some() {
                        entries.push(CommandEntry {
                            description: authored
                                .map(str::to_owned)
                                .unwrap_or_else(|| known::fallback(&path)),
                            name: name.clone(),
                        });
                    }

                    let fingerprint = fingerprint(&path, &metadata);
                    let job = DescriptionJob {
                        name: name.clone(),
                        path,
                        fingerprint,
                        authored_description,
                    };
                    if let Some(cached) = cache.entries.get(&name)
                        && cached.fingerprint == job.fingerprint
                        && cache_is_fresh(cached, now)
                    {
                        state.settled.insert(name.clone());
                        if !authored_description && let Some(description) = &cached.description {
                            state.descriptions.insert(name.clone(), description.clone());
                        }
                        if !cached.options.is_empty() {
                            state.options.insert(name.clone(), cached.options.clone());
                        }
                        if !cached.subcommands.is_empty() {
                            state
                                .subcommands
                                .insert(name.clone(), cached.subcommands.clone());
                        }
                        if !cached.option_values.is_empty() {
                            state
                                .option_values
                                .insert(name.clone(), cached.option_values.clone());
                        }
                    }
                    jobs.insert(name, job);
                }
            }
        }

        entries.sort_unstable_by(|left, right| left.name.cmp(&right.name));
        let state = Arc::new(Mutex::new(state));
        let queue = start_workers(Arc::clone(&state), cache_file, cache);
        Self {
            entries,
            jobs,
            state,
            queue,
        }
    }
}

fn start_workers(
    state: Arc<Mutex<EnrichmentState>>,
    cache_file: PathBuf,
    cache: super::DescriptionCache,
) -> Option<SyncSender<DescriptionJob>> {
    let (sender, receiver) = sync_channel(QUEUE_CAPACITY);
    let receiver = Arc::new(Mutex::new(receiver));
    let cache = Arc::new(Mutex::new(cache));
    let mut started = 0;

    for index in 0..WORKER_COUNT {
        let state = Arc::clone(&state);
        let receiver = Arc::clone(&receiver);
        let cache = Arc::clone(&cache);
        let cache_file = cache_file.clone();
        let worker = thread::Builder::new()
            .name(format!("zsuggestion-description-{index}"))
            .spawn(move || description_worker(&receiver, &state, &cache_file, &cache));
        if worker.is_ok() {
            started += 1;
        }
    }

    (started > 0).then_some(sender)
}

fn description_worker(
    receiver: &Mutex<Receiver<DescriptionJob>>,
    state: &Mutex<EnrichmentState>,
    cache_file: &Path,
    cache: &Mutex<super::DescriptionCache>,
) {
    loop {
        let job = {
            let receiver = receiver.lock().expect("description queue lock poisoned");
            receiver.recv()
        };
        let Ok(job) = job else {
            return;
        };

        let unchanged_before = fingerprint_matches(&job);
        let enrichment = if unchanged_before {
            discover_enrichment(&job, cache_file.parent().unwrap_or(Path::new("/tmp")))
        } else {
            super::Enrichment::default()
        };
        let cacheable = unchanged_before && fingerprint_matches(&job);
        {
            let mut state = state.lock().expect("command state lock poisoned");
            state.pending.remove(&job.name);
            state.settled.insert(job.name.clone());
            if cacheable {
                if !job.authored_description
                    && let Some(description) = &enrichment.description
                {
                    state
                        .descriptions
                        .insert(job.name.clone(), description.clone());
                }
                if !enrichment.options.is_empty() {
                    state
                        .options
                        .insert(job.name.clone(), enrichment.options.clone());
                }
                if !enrichment.subcommands.is_empty() {
                    state
                        .subcommands
                        .insert(job.name.clone(), enrichment.subcommands.clone());
                }
                if !enrichment.option_values.is_empty() {
                    state
                        .option_values
                        .insert(job.name.clone(), enrichment.option_values.clone());
                }
            }
        }

        if cacheable {
            let mut cache = cache.lock().expect("description cache lock poisoned");
            cache.entries.insert(
                job.name,
                super::CachedDescription {
                    fingerprint: job.fingerprint,
                    checked_at_secs: now_secs(),
                    description: enrichment.description,
                    options: enrichment.options,
                    subcommands: enrichment.subcommands,
                    option_values: enrichment.option_values,
                },
            );
            let _ = save_cache(cache_file, &cache);
        }
    }
}

pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.chars().any(char::is_control)
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'))
}
