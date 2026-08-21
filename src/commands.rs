use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const CACHE_VERSION: u32 = 4;
const CACHE_MAX_BYTES: u64 = 2 * 1024 * 1024;
const OUTPUT_MAX_BYTES: u64 = 64 * 1024;
const MAX_OPTION_COUNT: usize = 256;
const MAX_OPTION_SPELLING_BYTES: usize = 128;
const MAX_SUBCOMMAND_COUNT: usize = 256;
const MAX_SUBCOMMAND_NAME_BYTES: usize = 128;
const MAX_VALUE_COUNT: usize = 128;
const MAX_VALUE_BYTES: usize = 128;
const QUEUE_CAPACITY: usize = 64;
const WORKER_COUNT: usize = 2;
const SUCCESS_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const MISS_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const MAN_TIMEOUT: Duration = Duration::from_millis(1_500);
#[cfg(target_os = "macos")]
const HELP_TIMEOUT: Duration = Duration::from_millis(1_000);

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
    pub fn discover(cache_file: PathBuf) -> Self {
        let cache = load_cache(&cache_file);
        let now = now_secs();
        let mut seen = HashSet::new();
        let mut entries = Vec::new();
        let mut jobs = HashMap::new();
        let mut state = EnrichmentState::default();

        for (name, description) in SHELL_BUILTINS {
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
                    let authored = known_description(&name);
                    let authored_description = !seen.insert(name.clone()) || authored.is_some();
                    if !authored_description || authored.is_some() {
                        entries.push(CommandEntry {
                            description: authored
                                .map(str::to_owned)
                                .unwrap_or_else(|| fallback_description(&path)),
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

fn start_workers(
    state: Arc<Mutex<EnrichmentState>>,
    cache_file: PathBuf,
    cache: DescriptionCache,
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
            .name(format!("aster-description-{index}"))
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
    cache: &Mutex<DescriptionCache>,
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
            Enrichment::default()
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
                CachedDescription {
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

fn discover_enrichment(job: &DescriptionJob, output_dir: &Path) -> Enrichment {
    let mut enrichment = man_enrichment(&job.name, output_dir).unwrap_or_default();
    if (enrichment.description.is_none()
        || enrichment.options.is_empty()
        || enrichment.subcommands.is_empty()
        || enrichment.option_values.is_empty())
        && let Some(help) = help_enrichment(job, output_dir)
    {
        if enrichment.description.is_none() {
            enrichment.description = help.description;
        }
        if enrichment.options.is_empty() {
            enrichment.options = help.options;
        }
        if enrichment.subcommands.is_empty() {
            enrichment.subcommands = help.subcommands;
        }
        if enrichment.option_values.is_empty() {
            enrichment.option_values = help.option_values;
        }
    }
    enrichment
}

fn man_enrichment(name: &str, output_dir: &Path) -> Option<Enrichment> {
    let man = Path::new("/usr/bin/man");
    if !man.is_file() {
        return None;
    }
    let mut command = Command::new(man);
    command
        .arg("--")
        .arg(name)
        .env_clear()
        .env("HOME", "/nonexistent")
        .env("LC_ALL", "C")
        .env("MANPAGER", "cat")
        .env("PAGER", "cat");
    let output = run_bounded(command, output_dir, MAN_TIMEOUT)?;
    Some(Enrichment {
        description: parse_man_description(name, &output),
        options: parse_options(&output),
        subcommands: parse_subcommands(&output),
        option_values: parse_option_values(&output),
    })
}

#[cfg(target_os = "macos")]
fn help_enrichment(job: &DescriptionJob, output_dir: &Path) -> Option<Enrichment> {
    let sandbox = Path::new("/usr/bin/sandbox-exec");
    if !sandbox.is_file() {
        return None;
    }
    let mut command = Command::new(sandbox);
    command
        .arg("-p")
        .arg(
            "(version 1) (deny default) (allow process-exec) (allow file-read*) \
             (allow sysctl-read) (allow mach-lookup)",
        )
        .arg(&job.path)
        .arg("--help")
        .env_clear()
        .env("HOME", "/nonexistent")
        .env("LC_ALL", "C")
        .env("NO_COLOR", "1")
        .env("PAGER", "cat")
        .env("MANPAGER", "cat")
        .env("TERM", "dumb");
    let output = run_bounded(command, output_dir, HELP_TIMEOUT)?;
    Some(Enrichment {
        description: parse_help_description(&job.name, &output),
        options: parse_options(&output),
        subcommands: parse_subcommands(&output),
        option_values: parse_option_values(&output),
    })
}

#[cfg(not(target_os = "macos"))]
fn help_enrichment(_job: &DescriptionJob, _output_dir: &Path) -> Option<Enrichment> {
    None
}

fn run_bounded(mut command: Command, output_dir: &Path, timeout: Duration) -> Option<String> {
    let (path, mut output) = temporary_output(output_dir)?;
    let stdout = output.try_clone().ok()?;
    let stderr = output.try_clone().ok()?;
    command
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .process_group(0);
    unsafe {
        command.pre_exec(|| {
            let limit = libc::rlimit {
                rlim_cur: OUTPUT_MAX_BYTES,
                rlim_max: OUTPUT_MAX_BYTES,
            };
            if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            let _ = fs::remove_file(path);
            return None;
        }
    };
    let _ = fs::remove_file(&path);
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) | Err(_) => {
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.wait();

    output.seek(SeekFrom::Start(0)).ok()?;
    let mut bytes = Vec::new();
    output.take(OUTPUT_MAX_BYTES).read_to_end(&mut bytes).ok()?;
    String::from_utf8(bytes).ok()
}

fn temporary_output(directory: &Path) -> Option<(PathBuf, File)> {
    for _ in 0..10 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(
            ".description-output-{}-{sequence}",
            std::process::id()
        ));
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(&path);
        match file {
            Ok(file) => return Some((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

fn parse_man_description(name: &str, output: &str) -> Option<String> {
    let mut name_section = false;
    for line in clean_lines(output) {
        if line == "NAME" {
            name_section = true;
            continue;
        }
        if name_section {
            name_section = false;
            if let Some(description) = line.strip_prefix(name)
                && description.chars().next().is_some_and(char::is_whitespace)
                && let Some(description) =
                    sanitize_description(description.trim_start().trim_start_matches('-'))
            {
                return Some(description);
            }
        }
        let Some((names, description)) = line.split_once(" - ") else {
            continue;
        };
        let exact = names.split(',').any(|candidate| {
            candidate
                .trim()
                .strip_prefix(name)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('('))
        });
        if exact && let Some(description) = sanitize_description(description) {
            return Some(description);
        }
    }
    None
}

#[cfg(any(target_os = "macos", test))]
fn parse_help_description(name: &str, output: &str) -> Option<String> {
    for line in clean_lines(output) {
        let lower = line.to_ascii_lowercase();
        let structural = [
            "usage",
            "options",
            "commands",
            "arguments",
            "available commands",
            "flags",
            "examples",
        ]
        .iter()
        .any(|heading| lower == *heading || lower.starts_with(&format!("{heading}:")));
        let command_usage =
            lower.starts_with(&format!("{name} [")) || lower.starts_with(&format!("{name} <"));
        let diagnostic = [
            "error:",
            "warning:",
            "failed",
            "couldn't",
            "unrecognized option",
            "unknown option",
        ]
        .iter()
        .any(|text| lower.contains(text));
        if structural || command_usage || diagnostic || line.starts_with(['-', '[']) {
            continue;
        }
        if let Some(description) = sanitize_description(&line)
            && description.split_whitespace().count() >= 2
        {
            return Some(description);
        }
    }
    None
}

fn parse_options(output: &str) -> Vec<OptionMatch> {
    let mut entries: Vec<OptionMatch> = Vec::new();
    let mut in_options = false;
    let mut section_indent = 0;
    let mut continuation_indexes = Vec::new();
    let mut declaration_indent = 0;

    for raw_line in output.lines() {
        let Some(line) = clean_line(raw_line) else {
            continuation_indexes.clear();
            continue;
        };
        let text = line.trim();
        let indent = line.len() - line.trim_start_matches(' ').len();

        if is_options_heading(text) {
            if in_options {
                break;
            }
            in_options = true;
            section_indent = indent;
            continuation_indexes.clear();
            continue;
        }
        if !in_options {
            continue;
        }
        if indent <= section_indent && is_section_heading(text) {
            break;
        }

        if !option_line_has_unsafe_data(raw_line)
            && let Some(declaration) = parse_option_declaration(text)
        {
            continuation_indexes.clear();
            declaration_indent = indent;
            for spelling in declaration.spellings {
                if let Some(index) = entries
                    .iter()
                    .position(|option| option.spelling == spelling)
                {
                    if entries[index].description.is_empty()
                        && let Some(description) = &declaration.description
                    {
                        entries[index].description = description.clone();
                    }
                    continuation_indexes.push(index);
                } else if entries.len() < MAX_OPTION_COUNT {
                    entries.push(OptionMatch {
                        spelling,
                        description: declaration.description.clone().unwrap_or_default(),
                    });
                    continuation_indexes.push(entries.len() - 1);
                }
            }
            continue;
        }

        if !continuation_indexes.is_empty()
            && indent > declaration_indent
            && !text.starts_with('-')
            && !is_section_heading(text)
            && let Some(continuation) = sanitize_description(text)
        {
            for &index in &continuation_indexes {
                let combined = if entries[index].description.is_empty() {
                    continuation.clone()
                } else {
                    format!("{} {continuation}", entries[index].description)
                };
                if let Some(description) = sanitize_description(&combined) {
                    entries[index].description = description;
                }
            }
        } else {
            continuation_indexes.clear();
        }
    }

    entries
}

fn parse_subcommands(output: &str) -> Vec<SubcommandMatch> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    let mut in_commands = false;
    let mut section_indent = 0;
    let mut declaration_indent = None;

    for raw_line in output.lines() {
        let Some(line) = clean_line(raw_line) else {
            continue;
        };
        let text = line.trim();
        let indent = line.len() - line.trim_start_matches(' ').len();

        if is_commands_heading(text) {
            in_commands = true;
            section_indent = indent;
            declaration_indent = None;
            continue;
        }
        if !in_commands {
            continue;
        }
        if indent <= section_indent && is_section_heading(text) {
            in_commands = false;
            declaration_indent = None;
            continue;
        }
        if indent <= section_indent
            || option_line_has_unsafe_data(raw_line)
            || text.starts_with('-')
        {
            continue;
        }

        let Some((names, description)) = parse_subcommand_declaration(text) else {
            continue;
        };
        let expected_indent = *declaration_indent.get_or_insert(indent);
        if indent != expected_indent {
            continue;
        }
        for name in names {
            if seen.insert(name.clone()) {
                entries.push(SubcommandMatch {
                    name,
                    description: description.clone(),
                });
                if entries.len() >= MAX_SUBCOMMAND_COUNT {
                    return entries;
                }
            }
        }
    }

    entries
}

fn parse_subcommand_declaration(line: &str) -> Option<(Vec<String>, String)> {
    let bytes = line.as_bytes();
    let column = bytes
        .windows(2)
        .position(|pair| pair[0].is_ascii_whitespace() && pair[1].is_ascii_whitespace());
    let (declaration, description) = if let Some(column) = column {
        (&line[..column], line[column..].trim())
    } else if line.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return None;
    } else {
        (line, "")
    };
    let names: Vec<_> = declaration
        .split(',')
        .map(|name| name.trim().trim_end_matches([':', '*']))
        .filter(|name| valid_subcommand_name(name))
        .map(str::to_owned)
        .collect();
    if names.is_empty() {
        return None;
    }
    Some((names, sanitize_description(description).unwrap_or_default()))
}

fn parse_option_values(output: &str) -> Vec<OptionValues> {
    let mut entries = Vec::new();
    let mut in_options = false;
    let mut section_indent = 0;
    let mut declaration_indent = 0;
    let mut current_options = Vec::new();
    let mut collecting_list = false;

    for raw_line in output.lines() {
        let Some(line) = clean_line(raw_line) else {
            collecting_list = false;
            continue;
        };
        let text = line.trim();
        let indent = line.len() - line.trim_start_matches(' ').len();

        if is_options_heading(text) {
            if in_options {
                break;
            }
            in_options = true;
            section_indent = indent;
            current_options.clear();
            continue;
        }
        if !in_options {
            continue;
        }
        if indent <= section_indent && is_section_heading(text) {
            break;
        }
        if option_line_has_unsafe_data(raw_line) {
            current_options.clear();
            collecting_list = false;
            continue;
        }

        if let Some(declaration) = parse_option_declaration(text) {
            current_options = declaration.spellings;
            declaration_indent = indent;
            let (values, list_follows) = documented_values(text);
            insert_option_values(&mut entries, &current_options, values);
            collecting_list = list_follows;
            continue;
        }

        if current_options.is_empty() || indent <= declaration_indent {
            collecting_list = false;
            continue;
        }
        let (values, list_follows) = documented_values(text);
        if !values.is_empty() || list_follows {
            insert_option_values(&mut entries, &current_options, values);
            collecting_list = list_follows;
        } else if collecting_list {
            if let Some(value) = documented_value_bullet(text) {
                insert_option_values(&mut entries, &current_options, vec![value]);
            } else {
                collecting_list = false;
            }
        }
    }

    entries
}

fn insert_option_values(
    entries: &mut Vec<OptionValues>,
    options: &[String],
    values: Vec<ValueMatch>,
) {
    if values.is_empty() {
        return;
    }
    for option in options {
        let index = entries
            .iter()
            .position(|entry| entry.option == *option)
            .unwrap_or_else(|| {
                entries.push(OptionValues {
                    option: option.clone(),
                    values: Vec::new(),
                });
                entries.len() - 1
            });
        for value in &values {
            if entries[index].values.len() >= MAX_VALUE_COUNT {
                break;
            }
            if let Some(existing) = entries[index]
                .values
                .iter_mut()
                .find(|existing| existing.value == value.value)
            {
                if existing.description.is_empty() {
                    existing.description = value.description.clone();
                }
            } else {
                entries[index].values.push(value.clone());
            }
        }
    }
}

fn documented_values(line: &str) -> (Vec<ValueMatch>, bool) {
    let lower = line.to_ascii_lowercase();
    for marker in ["possible values:", "possible value:"] {
        if let Some(index) = lower.find(marker) {
            let remainder = line[index + marker.len()..]
                .trim()
                .trim_matches(|character| matches!(character, '[' | ']' | '(' | ')'))
                .trim();
            if remainder.is_empty() {
                return (Vec::new(), true);
            }
            return (parse_value_list(remainder, ','), false);
        }
    }

    let syntax_end = line
        .as_bytes()
        .windows(2)
        .position(|pair| pair[0].is_ascii_whitespace() && pair[1].is_ascii_whitespace())
        .unwrap_or(line.len());
    let syntax = &line[..syntax_end];
    for (open, close, separator) in [('{', '}', ','), ('[', ']', '|'), ('<', '>', '|')] {
        let Some(start) = syntax.find(open) else {
            continue;
        };
        let Some(end_offset) = syntax[start + 1..].find(close) else {
            continue;
        };
        let values = &syntax[start + 1..start + 1 + end_offset];
        if values.contains(separator) {
            return (parse_value_list(values, separator), false);
        }
    }
    (Vec::new(), false)
}

fn parse_value_list(values: &str, separator: char) -> Vec<ValueMatch> {
    values
        .split(separator)
        .filter_map(|value| {
            let value = value
                .trim()
                .trim_matches(|character| matches!(character, '\'' | '"' | '`'));
            valid_value(value).then(|| ValueMatch {
                value: value.to_owned(),
                description: String::new(),
            })
        })
        .take(MAX_VALUE_COUNT)
        .collect()
}

fn documented_value_bullet(line: &str) -> Option<ValueMatch> {
    let line = line.strip_prefix('-')?.trim_start();
    let (value, description) = line
        .split_once(':')
        .map_or((line, ""), |(value, description)| {
            (value.trim(), description.trim())
        });
    let value = value.trim_matches(|character| matches!(character, '\'' | '"' | '`'));
    valid_value(value).then(|| ValueMatch {
        value: value.to_owned(),
        description: sanitize_description(description).unwrap_or_default(),
    })
}

struct ParsedOptionDeclaration {
    spellings: Vec<String>,
    description: Option<String>,
}

fn parse_option_declaration(line: &str) -> Option<ParsedOptionDeclaration> {
    if !line.starts_with('-') {
        return None;
    }
    let bytes = line.as_bytes();
    let mut position = 0;
    let mut spellings = Vec::new();

    loop {
        let (spelling, end) = option_spelling_at(line, position)?;
        spellings.push(spelling.to_owned());
        position = end;

        if bytes.get(position) == Some(&b'=') {
            position += 1;
            let argument_start = position;
            while bytes
                .get(position)
                .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b',')
            {
                position += 1;
            }
            if position == argument_start {
                return None;
            }
        } else if bytes.get(position) == Some(&b'[') {
            let argument_end = bytes[position..].iter().position(|byte| *byte == b']')? + position;
            let argument = &line[position + 1..argument_end];
            if !argument.strip_prefix('=').is_some_and(is_option_argument) {
                return None;
            }
            position = argument_end + 1;
        }

        if bytes.get(position) == Some(&b',') {
            position += 1;
            skip_ascii_spaces(bytes, &mut position);
            if bytes.get(position) == Some(&b'-') {
                continue;
            }
            return None;
        }

        if position == bytes.len() {
            break;
        }
        if !bytes[position].is_ascii_whitespace() {
            return None;
        }
        skip_ascii_spaces(bytes, &mut position);
        if position == bytes.len() {
            break;
        }
        if bytes[position] == b'/' {
            position += 1;
            skip_ascii_spaces(bytes, &mut position);
            continue;
        }
        if bytes[position] == b'-' {
            continue;
        }

        let token_end = bytes[position..]
            .iter()
            .position(u8::is_ascii_whitespace)
            .map_or(bytes.len(), |offset| position + offset);
        let argument = line[position..token_end].trim_end_matches(',');
        if is_option_argument(argument) {
            position = token_end;
            skip_ascii_spaces(bytes, &mut position);
            if line[..token_end].ends_with(',') {
                continue;
            }
        }
        let description = (position < bytes.len())
            .then(|| sanitize_description(&line[position..]))
            .flatten();
        return Some(ParsedOptionDeclaration {
            spellings,
            description,
        });
    }

    Some(ParsedOptionDeclaration {
        spellings,
        description: None,
    })
}

fn option_spelling_at(line: &str, position: usize) -> Option<(&str, usize)> {
    let bytes = line.as_bytes();
    if bytes.get(position) != Some(&b'-') {
        return None;
    }
    let mut end = position + 1;
    if bytes.get(end) == Some(&b'-') {
        end += 1;
        let name_start = end;
        while bytes
            .get(end)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            end += 1;
        }
        if end == name_start {
            return None;
        }
    } else {
        if !bytes.get(end).is_some_and(u8::is_ascii_alphanumeric) {
            return None;
        }
        end += 1;
    }

    let spelling = &line[position..end];
    let delimiter = bytes.get(end);
    if !valid_option_spelling(spelling)
        || delimiter
            .is_some_and(|byte| !byte.is_ascii_whitespace() && !matches!(byte, b',' | b'=' | b'['))
    {
        return None;
    }
    Some((spelling, end))
}

fn valid_option_spelling(spelling: &str) -> bool {
    if !spelling.is_ascii()
        || spelling.len() > MAX_OPTION_SPELLING_BYTES
        || !spelling.starts_with('-')
    {
        return false;
    }
    let bytes = spelling.as_bytes();
    if bytes.get(1) == Some(&b'-') {
        bytes.len() >= 3
            && bytes[2].is_ascii_alphanumeric()
            && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
            && bytes[2..]
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    } else {
        bytes.len() == 2 && bytes[1].is_ascii_alphanumeric()
    }
}

fn option_line_has_unsafe_data(line: &str) -> bool {
    let characters: Vec<_> = line.chars().collect();
    characters.iter().enumerate().any(|(index, &character)| {
        if is_directional_format(character) {
            return true;
        }
        if character == '\u{8}' {
            return index == 0
                || index + 1 == characters.len()
                || (characters[index - 1] != characters[index + 1]
                    && characters[index - 1] != '_');
        }
        character.is_control() && character != '\t'
    })
}

fn is_option_argument(token: &str) -> bool {
    let token = token
        .strip_prefix('<')
        .and_then(|token| token.strip_suffix('>'))
        .or_else(|| {
            token
                .strip_prefix('[')
                .and_then(|token| token.strip_suffix(']'))
        })
        .unwrap_or(token)
        .trim_end_matches("...");
    !token.is_empty()
        && token.is_ascii()
        && token
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

fn skip_ascii_spaces(bytes: &[u8], position: &mut usize) {
    while bytes.get(*position).is_some_and(u8::is_ascii_whitespace) {
        *position += 1;
    }
}

fn is_options_heading(line: &str) -> bool {
    matches!(
        line.trim_end_matches(':').to_ascii_lowercase().as_str(),
        "option"
            | "options"
            | "flags"
            | "global options"
            | "general options"
            | "optional arguments"
            | "the following options are available"
    )
}

fn is_commands_heading(line: &str) -> bool {
    let has_colon = line.ends_with(':');
    let heading = line.trim_end_matches(':');
    let lower = heading.to_ascii_lowercase();
    let heading_shape = has_colon
        || heading
            .chars()
            .filter(|character| character.is_ascii_alphabetic())
            .all(|character| character.is_ascii_uppercase());
    matches!(
        lower.as_str(),
        "command" | "commands" | "available commands" | "subcommands" | "the commands are"
    ) || heading_shape && lower.ends_with(" commands")
}

fn is_section_heading(line: &str) -> bool {
    let heading = line.trim_end_matches(':');
    let lower = heading.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "usage"
            | "arguments"
            | "commands"
            | "available commands"
            | "examples"
            | "description"
            | "synopsis"
            | "operands"
            | "environment"
            | "exit status"
            | "files"
            | "authors"
            | "bugs"
            | "see also"
    ) {
        return true;
    }
    heading
        .chars()
        .any(|character| character.is_ascii_alphabetic())
        && heading
            .chars()
            .all(|character| !character.is_ascii_alphabetic() || character.is_ascii_uppercase())
}

fn clean_lines(output: &str) -> impl Iterator<Item = String> + '_ {
    output
        .lines()
        .filter_map(clean_line)
        .map(|line| line.trim().to_owned())
}

fn clean_line(line: &str) -> Option<String> {
    let mut clean = String::with_capacity(line.len());
    let mut escape = 0;
    for character in line.chars() {
        if escape == 1 {
            escape = match character {
                '[' => 2,
                ']' => 3,
                _ => 0,
            };
            continue;
        }
        if escape == 2 {
            if ('@'..='~').contains(&character) {
                escape = 0;
            }
            continue;
        }
        if escape == 3 {
            continue;
        }
        if character == '\u{1b}' {
            escape = 1;
        } else if character == '\u{8}' {
            clean.pop();
        } else if character == '\t' {
            clean.push(' ');
        } else if !character.is_control() && !is_directional_format(character) {
            clean.push(character);
        }
    }
    let clean = clean.trim_end().to_owned();
    (!clean.trim().is_empty()).then_some(clean)
}

fn sanitize_description(description: &str) -> Option<String> {
    let mut clean = description.split_whitespace().collect::<Vec<_>>().join(" ");
    clean.retain(|character| !character.is_control() && !is_directional_format(character));
    if clean.is_empty() || !clean.chars().any(char::is_alphabetic) {
        return None;
    }
    if clean.chars().count() > 200 {
        clean = clean.chars().take(199).collect();
        clean.push('…');
    }
    Some(clean)
}

fn is_directional_format(character: char) -> bool {
    matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

fn load_cache(path: &Path) -> DescriptionCache {
    let Ok(metadata) = fs::metadata(path) else {
        return DescriptionCache::default();
    };
    if metadata.len() > CACHE_MAX_BYTES {
        return DescriptionCache::default();
    }
    let Ok(bytes) = fs::read(path) else {
        return DescriptionCache::default();
    };
    let Ok(mut cache) = serde_json::from_slice::<DescriptionCache>(&bytes) else {
        return DescriptionCache::default();
    };
    if cache.version != CACHE_VERSION {
        return DescriptionCache::default();
    }
    for cached in cache.entries.values_mut() {
        let mut seen = HashSet::new();
        cached.options.retain_mut(|option| {
            if !valid_option_spelling(&option.spelling) || !seen.insert(option.spelling.clone()) {
                return false;
            }
            option.description = sanitize_description(&option.description).unwrap_or_default();
            true
        });
        cached.options.truncate(MAX_OPTION_COUNT);

        seen.clear();
        cached.subcommands.retain_mut(|subcommand| {
            if !valid_subcommand_name(&subcommand.name) || !seen.insert(subcommand.name.clone()) {
                return false;
            }
            subcommand.description =
                sanitize_description(&subcommand.description).unwrap_or_default();
            true
        });
        cached.subcommands.truncate(MAX_SUBCOMMAND_COUNT);

        seen.clear();
        cached.option_values.retain_mut(|option_values| {
            if !valid_option_spelling(&option_values.option)
                || !seen.insert(option_values.option.clone())
            {
                return false;
            }
            let mut seen_values = HashSet::new();
            option_values.values.retain_mut(|value| {
                if !valid_value(&value.value) || !seen_values.insert(value.value.clone()) {
                    return false;
                }
                value.description = sanitize_description(&value.description).unwrap_or_default();
                true
            });
            option_values.values.truncate(MAX_VALUE_COUNT);
            !option_values.values.is_empty()
        });
    }
    cache
}

fn save_cache(path: &Path, cache: &DescriptionCache) -> std::io::Result<()> {
    let mut bounded = cache.clone();
    let bytes = loop {
        let bytes = serde_json::to_vec(&bounded)?;
        if bytes.len() as u64 <= CACHE_MAX_BYTES || bounded.entries.is_empty() {
            break bytes;
        }
        let Some(oldest) = bounded
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.checked_at_secs)
            .map(|(name, _)| name.clone())
        else {
            break bytes;
        };
        bounded.entries.remove(&oldest);
    };
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("tmp-{}-{sequence}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn cache_is_fresh(cached: &CachedDescription, now: u64) -> bool {
    let ttl = if !cached.options.is_empty()
        || !cached.subcommands.is_empty()
        || !cached.option_values.is_empty()
    {
        SUCCESS_TTL
    } else {
        MISS_TTL
    };
    now >= cached.checked_at_secs && now - cached.checked_at_secs <= ttl.as_secs()
}

fn fingerprint(path: &Path, metadata: &fs::Metadata) -> ExecutableFingerprint {
    ExecutableFingerprint {
        path: path.to_path_buf(),
        size: metadata.len(),
        device: metadata.dev(),
        inode: metadata.ino(),
        mode: metadata.mode(),
        modified_secs: metadata.mtime(),
        modified_nanos: metadata.mtime_nsec(),
        changed_secs: metadata.ctime(),
        changed_nanos: metadata.ctime_nsec(),
    }
}

fn fingerprint_matches(job: &DescriptionJob) -> bool {
    fs::metadata(&job.path)
        .map(|metadata| fingerprint(&job.path, &metadata) == job.fingerprint)
        .unwrap_or(false)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.chars().any(char::is_control)
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'))
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

fn fallback_description(path: &Path) -> String {
    let path = path.to_string_lossy();
    if path.contains("/.cargo/bin/") {
        "Executable installed by Cargo".to_owned()
    } else if path.contains("/homebrew/") || path.contains("/Cellar/") {
        "Homebrew command".to_owned()
    } else if path.contains("/.local/bin/") || path.contains("/bin/") && path.contains("/Users/") {
        "User-installed command".to_owned()
    } else {
        "System command".to_owned()
    }
}

fn known_description(name: &str) -> Option<&'static str> {
    Some(match name {
        "ansible" => "Define and run automation tasks",
        "appwrite" => "Manage Appwrite projects and services",
        "arch" => "Print architecture type or run a universal binary",
        "asr" => "Apple Software Restore; copy volumes and disk images",
        "atlas" => "CLI tool to manage MongoDB Atlas",
        "aws" => "Official command line interface for Amazon Web Services",
        "aws-vault" => "Securely store and access AWS credentials",
        "bash" => "GNU Bourne Again shell",
        "brew" => "The missing package manager for macOS",
        "cargo" => "Rust package manager and build tool",
        "cmake" => "Configure, build, and test software projects",
        "code" => "Open Visual Studio Code",
        "curl" => "Transfer data from or to a server",
        "docker" => "Build and run applications in containers",
        "fd" => "Fast and user-friendly file finder",
        "fzf" => "Command-line fuzzy finder",
        "gh" => "GitHub command line interface",
        "git" => "Distributed version control system",
        "go" => "Build and manage Go source code",
        "iris" => "Interactive shell assistant",
        "jq" => "Process and transform JSON",
        "kubectl" => "Control Kubernetes clusters",
        "make" => "Maintain and build groups of programs",
        "node" => "Run JavaScript with Node.js",
        "npm" => "JavaScript package manager",
        "nvim" => "Edit text with Neovim",
        "pnpm" => "Fast, disk-efficient JavaScript package manager",
        "python" | "python3" => "Run the Python interpreter",
        "rg" => "Recursively search files with ripgrep",
        "rustc" => "Compile Rust source code",
        "ssh" => "OpenSSH remote login client",
        "tmux" => "Terminal multiplexer",
        "yarn" => "JavaScript package manager",
        "zsh" => "Z shell command interpreter",
        _ => return None,
    })
}

const SHELL_BUILTINS: &[(&str, &str)] = &[
    ("alias", "Define or display shell aliases"),
    ("autoload", "Mark shell functions for automatic loading"),
    ("bg", "Resume jobs in the background"),
    ("cd", "Change the current working directory"),
    ("command", "Execute a command without shell function lookup"),
    ("export", "Set environment variables for child processes"),
    ("fg", "Bring jobs into the foreground"),
    ("jobs", "Display active shell jobs"),
    ("setopt", "Enable Zsh options"),
    (
        "source",
        "Execute commands from a file in the current shell",
    ),
    ("typeset", "Declare shell variables and attributes"),
    ("unalias", "Remove shell alias definitions"),
    ("unset", "Remove shell variables or functions"),
    ("unsetopt", "Disable Zsh options"),
];

#[cfg(test)]
mod tests {
    use super::*;
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
    fn parses_exact_man_description() {
        let output = "assetutil(1) - process asset catalog.car files\n\
                      other(1) - unrelated\n";
        assert_eq!(
            parse_man_description("assetutil", output).as_deref(),
            Some("process asset catalog.car files")
        );
        assert_eq!(parse_man_description("asset", output), None);
    }

    #[test]
    fn parses_overstruck_man_name_section() {
        let output = "N\u{8}NA\u{8}AM\u{8}ME\u{8}E\n       a\u{8}as\u{8}s - assembler\n";
        assert_eq!(
            parse_man_description("as", output).as_deref(),
            Some("assembler")
        );
    }

    #[test]
    fn parses_man_name_section_without_separator() {
        let output = "NAME\n     assetutil process asset catalog files\n\nSYNOPSIS\n";
        assert_eq!(
            parse_man_description("assetutil", output).as_deref(),
            Some("process asset catalog files")
        );
    }

    #[test]
    fn parses_prose_from_help_output() {
        let output =
            "Usage: tool [OPTIONS]\n\nInspect a project without changing it.\n\nOptions:\n";
        assert_eq!(
            parse_help_description("tool", output).as_deref(),
            Some("Inspect a project without changing it.")
        );
    }

    #[test]
    fn rejects_help_diagnostics() {
        let output = "tool: error: couldn't create cache file\nUsage: tool [OPTIONS]\n";
        assert_eq!(parse_help_description("tool", output), None);
    }

    #[test]
    fn parses_rendered_man_option_declarations_and_continuations() {
        let output = "NAME\n    tool - inspect things\nThe following options are available:\n\
                      \x20   -a, --all\n\
                      \x20       Include hidden entries.\n\
                      \x20   -o, --output FILE  Write to FILE.\n\
                      \x20       --color=WHEN    Control colored output.\nARGUMENTS\n\
                      \x20   FILE  Input file.\n";

        assert_eq!(
            parse_options(output),
            [
                OptionMatch {
                    spelling: "-a".to_owned(),
                    description: "Include hidden entries.".to_owned(),
                },
                OptionMatch {
                    spelling: "--all".to_owned(),
                    description: "Include hidden entries.".to_owned(),
                },
                OptionMatch {
                    spelling: "-o".to_owned(),
                    description: "Write to FILE.".to_owned(),
                },
                OptionMatch {
                    spelling: "--output".to_owned(),
                    description: "Write to FILE.".to_owned(),
                },
                OptionMatch {
                    spelling: "--color".to_owned(),
                    description: "Control colored output.".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn parses_common_help_option_sections() {
        let clap = "Usage: tool [OPTIONS]\n\nOptions:\n  -q, --quiet       Suppress output\n\
                    \x20     --format <FORMAT>  Select a format\n";
        let cobra = "Flags:\n  -h, --help   help for tool\nCommands:\n  child\n";
        let click = "Options:\n  --color / --no-color  Toggle color.\n  --help                  Show this message.\n";
        let argparse = "optional arguments:\n  -v, --verbose    increase verbosity\n  -o OUTPUT, --output OUTPUT  destination\n  --color[=WHEN]  color mode\n";

        assert_eq!(
            parse_options(clap)
                .into_iter()
                .map(|option| option.spelling)
                .collect::<Vec<_>>(),
            ["-q", "--quiet", "--format"]
        );
        assert_eq!(
            parse_options(cobra),
            [
                OptionMatch {
                    spelling: "-h".to_owned(),
                    description: "help for tool".to_owned(),
                },
                OptionMatch {
                    spelling: "--help".to_owned(),
                    description: "help for tool".to_owned(),
                },
            ]
        );
        assert_eq!(parse_options(click).len(), 3);
        assert_eq!(
            parse_options(argparse)
                .into_iter()
                .map(|option| option.spelling)
                .collect::<Vec<_>>(),
            ["-v", "--verbose", "-o", "--output", "--color"]
        );
    }

    #[test]
    fn parses_explicit_subcommand_sections() {
        let output = "Usage: tool [COMMAND]\n\nAvailable Commands:\n\
                      \x20  build, b  Build the project\n\
                      \x20  inspect   Inspect project state\n\
                      \x20    continuation text that is not a command\n\nOptions:\n\
                      \x20  --help    Print help\n\nAdditional Commands:\n\
                      \x20  deploy:   Deploy the project\n";

        assert_eq!(
            parse_subcommands(output),
            [
                SubcommandMatch {
                    name: "build".to_owned(),
                    description: "Build the project".to_owned(),
                },
                SubcommandMatch {
                    name: "b".to_owned(),
                    description: "Build the project".to_owned(),
                },
                SubcommandMatch {
                    name: "inspect".to_owned(),
                    description: "Inspect project state".to_owned(),
                },
                SubcommandMatch {
                    name: "deploy".to_owned(),
                    description: "Deploy the project".to_owned(),
                },
            ]
        );

        let prose = "Tool for running commands\n\
                     \x20  This paragraph is not a command declaration.\n\nCommands:\n\
                     \x20  valid  A real command\n\
                     \x20  Commands can be abbreviated\n";
        assert_eq!(
            parse_subcommands(prose),
            [SubcommandMatch {
                name: "valid".to_owned(),
                description: "A real command".to_owned(),
            }]
        );
    }

    #[test]
    fn parses_documented_option_values_for_every_alias() {
        let output = "Options:\n\
                      \x20  -c, --color <WHEN>  Color output [possible values: auto, always, never]\n\
                      \x20  --format {json,yaml}  Output format\n\
                      \x20  --mode <fast|safe>    Execution mode\n\
                      \x20  --template TEMPLATE  Expand {name,id} placeholders\n\
                      \x20  --target <TARGET>\n\
                      \x20      Possible values:\n\
                      \x20      - local: Run locally\n\
                      \x20      - remote: Run remotely\n";
        let values = parse_option_values(output);

        for option in ["-c", "--color"] {
            assert_eq!(
                values
                    .iter()
                    .find(|entry| entry.option == option)
                    .unwrap()
                    .values
                    .iter()
                    .map(|value| value.value.as_str())
                    .collect::<Vec<_>>(),
                ["auto", "always", "never"]
            );
        }
        assert_eq!(values_for(&values, "--format"), ["json", "yaml"]);
        assert_eq!(values_for(&values, "--mode"), ["fast", "safe"]);
        assert!(!values.iter().any(|entry| entry.option == "--template"));
        assert_eq!(values_for(&values, "--target"), ["local", "remote"]);
        assert_eq!(
            values
                .iter()
                .find(|entry| entry.option == "--target")
                .unwrap()
                .values[0]
                .description,
            "Run locally"
        );
    }

    #[test]
    fn rejects_usage_prose_subcommands_and_malicious_option_spellings() {
        let output = "Usage: tool --usage-only\n\
                      \x20Prose mentions --prose-only but is not an option.\n\
                      \x20Options:\n\
                      \x20  --safe        Safe option.\n\
                      \x20  --bad;touch   Not safe.\n\
                      \x20  --also$(evil) Not safe.\n\
                      \x20  --con\u{7}trol Control data.\n\
                      \x20  -abc          Combined spelling.\n\
                      \x20  —lookalike    Unicode dash.\n\
                      \x20Commands:\n\
                      \x20  child\n\
                      \x20Options:\n\
                      \x20  --child-only  Child option.\n";

        assert_eq!(
            parse_options(output),
            [OptionMatch {
                spelling: "--safe".to_owned(),
                description: "Safe option.".to_owned(),
            }]
        );
        assert!(!option_line_has_unsafe_data("-\u{8}-, h\u{8}h"));
    }

    #[test]
    fn strips_terminal_controls_from_descriptions() {
        let output = "tool(1) - \u{1b}[31mred\u{1b}[0m\u{202e} text\n";
        assert_eq!(
            parse_man_description("tool", output).as_deref(),
            Some("red text")
        );
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

    #[test]
    fn description_process_has_a_hard_timeout() {
        let directory = tempdir().unwrap();
        let mut command = Command::new("/bin/sleep");
        command.arg("2");
        let started = Instant::now();

        assert_eq!(
            run_bounded(command, directory.path(), Duration::from_millis(30)).as_deref(),
            Some("")
        );
        assert!(started.elapsed() < Duration::from_secs(1));
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

    fn values_for<'a>(values: &'a [OptionValues], option: &str) -> Vec<&'a str> {
        values
            .iter()
            .find(|entry| entry.option == option)
            .unwrap()
            .values
            .iter()
            .map(|value| value.value.as_str())
            .collect()
    }
}
