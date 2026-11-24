//! Recorded-history candidates.
//!
//! History is always the lowest presentation tier. Candidates carry the
//! complete suffix because the frontend replaces the typed baseline in one
//! step; an exact history match is not a useful completion and is skipped.
//!
//! History is recorded across directories, so an entry may name paths that do
//! not exist from the current working directory. Entries whose not-yet-typed
//! words reference such paths are dropped rather than offered as completions.

use std::path::Path;

use anyhow::Result;

use super::filesystem::{
    current_argument_start, explicit_path_token, resolve_directory, unescape_path_token,
};
use super::paths;
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
    // Over-fetch so entries rejected for missing paths do not starve the tier.
    let fetch = limit.saturating_mul(4).min(256).max(limit);
    for history in store.history_candidates(prefix, cwd, fetch, successful_first)? {
        let Some(suffix) = history.command.strip_prefix(prefix) else {
            continue;
        };
        if suffix.is_empty() || !paths_plausible(prefix, &history.command, cwd) {
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

/// Returns whether every word of `command` past the typed `prefix` that is a
/// path argument resolves from `cwd`.
///
/// Words already typed are the user's own and are not judged. Commands
/// containing unquoted shell syntax the tokenizer does not model (comments,
/// expansions, separators) are accepted conservatively.
fn paths_plausible(prefix: &str, command: &str, cwd: &str) -> bool {
    if paths::has_unquoted_syntax(command) {
        return true;
    }
    let first_checked = current_argument_start(prefix).unwrap_or(0);
    let words = shell_words(command);
    let Some((_, first_word)) = words.first() else {
        return true;
    };

    // A wrapper's target is a command name, not one of the wrapper's
    // arguments. Skip wrapper words so they are never judged as paths.
    let wrapper = matches!(first_word.as_str(), "sudo" | "command" | "exec" | "nohup");
    let command_index = if wrapper {
        words
            .iter()
            .enumerate()
            .skip(1)
            .find_map(|(index, (_, word))| (!word.starts_with('-')).then_some(index))
            .unwrap_or(0)
    } else {
        0
    };
    let command_word = &words[command_index].1;
    let force_remove = command_word == "rm"
        && words
            .iter()
            .any(|(_, word)| option_matches(&["-f", "--force"], word) || short_flag(word, 'f'));
    let directories_only = paths::takes_directories_only(command_word);
    let may_not_exist = paths::paths_may_not_exist(command_word);
    if force_remove || (may_not_exist && !directories_only) {
        return true;
    }
    let path_command = paths::is_path_command(command_word);
    let script_command = paths::is_script_command(command_word);
    let leading_non_paths = leading_non_paths(command_word);

    let mut positional = 0usize;
    let mut skip_value = false;
    for (index, (start, word)) in words.iter().enumerate() {
        if index < command_index {
            continue;
        }
        if index == command_index {
            if *start >= first_checked && !word_plausible(word, true, path_command, cwd) {
                return false;
            }
            continue;
        }
        if skip_value {
            skip_value = false;
            continue;
        }
        if word.starts_with('-') {
            if option_ends_path_check(command_word, word) {
                return true;
            }
            skip_value = option_takes_value(command_word, word);
            continue;
        }
        positional += 1;
        if positional <= leading_non_paths {
            // chmod's mode and chown's owner are positional but not paths.
            continue;
        }
        if script_command && positional > 1 {
            // Interpreter arguments after the script are the script's own,
            // not paths that must exist.
            continue;
        }
        if *start >= first_checked {
            if !may_not_exist && !word_plausible(word, false, path_command, cwd) {
                return false;
            }
            if directories_only && !directory_plausible(word, cwd) {
                return false;
            }
        }
    }
    true
}

fn word_plausible(word: &str, command_position: bool, path_command: bool, cwd: &str) -> bool {
    if word.is_empty()
        || word.starts_with('-')
        || word.starts_with('+')
        || word.contains(['=', '@', ':'])
    {
        return true;
    }
    if word.starts_with('~') && !(word == "~" || word.starts_with("~/")) {
        return true; // ~user and named directories are not resolved here.
    }
    let explicit = word == "~" || word == "." || word == ".." || explicit_path_token(word);
    let path = resolve_path(word, cwd);
    if path.exists() {
        return true;
    }
    if explicit {
        return false;
    }
    if command_position {
        // Plain command names are resolved through PATH, not the filesystem.
        return true;
    }
    if path_command {
        return false;
    }
    if !word.contains('/') {
        return true;
    }
    let parent_exists = path
        .parent()
        .is_some_and(|parent| parent != Path::new(cwd) && parent.is_dir());
    if parent_exists {
        return false;
    }
    // Neither the path nor its parent exists. A word shaped like a file path
    // is almost certainly relative to another directory (or stale);
    // slash-separated names such as branches stay acceptable.
    let file_shaped = word.ends_with('/')
        || word
            .rsplit('/')
            .next()
            .and_then(|name| name.rsplit_once('.'))
            .is_some_and(|(stem, extension)| {
                !stem.is_empty()
                    && (1..=8).contains(&extension.len())
                    && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
                    && extension.bytes().any(|byte| byte.is_ascii_alphabetic())
            });
    !file_shaped
}

/// Rejects an existing path that cannot be a directory for a directory-only
/// command (`cd README.md`, `rmdir src/main.rs`). Missing paths are already
/// handled by [`word_plausible`], so they stay accepted here.
fn directory_plausible(word: &str, cwd: &str) -> bool {
    if word.is_empty()
        || word.starts_with('-')
        || word.starts_with('+')
        || word.contains(['=', '@', ':'])
    {
        return true;
    }
    if word.starts_with('~') && !(word == "~" || word.starts_with("~/")) {
        return true; // ~user and named directories are not resolved here.
    }
    let path = resolve_path(word, cwd);
    !path.exists() || path.is_dir()
}

/// Leading positional arguments that are not paths.
fn leading_non_paths(command: &str) -> usize {
    match command {
        "chmod" | "chown" => 1,
        _ => 0,
    }
}

/// Option flags that consume a separate value which is not a path.
fn value_options(command: &str) -> &'static [&'static str] {
    match command {
        "ls" => &[
            "-I",
            "--ignore",
            "-T",
            "--tabsize",
            "-w",
            "--width",
            "--format",
            "--time-style",
            "--quoting-style",
            "--sort",
            "--block-size",
            "--color",
            "--hyperlink",
            "--indicator-style",
        ],
        "mkdir" => &["-m", "--mode", "-Z", "--context"],
        "less" => &[
            "-b", "-h", "-j", "-k", "-o", "-O", "-p", "-P", "-D", "-t", "-T", "-x", "-y", "-z",
            "--shift", "--tabs", "--window", "--quotes",
        ],
        "more" => &["-n", "--lines", "-p", "--prompt"],
        "head" | "tail" => &[
            "-n",
            "--lines",
            "-c",
            "--bytes",
            "-s",
            "--sleep-interval",
            "--pid",
        ],
        "bat" => &[
            "--theme",
            "--language",
            "--style",
            "--line-range",
            "--terminal-width",
            "--tabs",
            "--wrap",
            "--paging",
            "--decorations",
            "--color",
            "--italic-text",
            "--highlight-line",
            "--file-name",
            "--diff-context",
            "--map-syntax",
            "--ignored-suffix",
            "--nonprintable-notation",
            "--pager",
            "--config-file",
            "--config-path",
        ],
        "open" => &["-a", "--app", "-b", "--bundle"],
        "file" => &[
            "-f",
            "--files-from",
            "-m",
            "--magic-file",
            "-F",
            "--separator",
            "--exclude",
            "--exclude-quiet",
            "-e",
            "-P",
            "--parameter",
        ],
        "stat" => &["-c", "--format", "--printf"],
        "diff" => &[
            "-x",
            "--exclude",
            "-X",
            "--exclude-from",
            "-I",
            "--ignore-matching-lines",
            "--label",
            "--starting-file",
            "-C",
            "--context",
            "-U",
            "--unified",
            "-W",
            "--width",
            "--tabsize",
            "--horizon-lines",
            "--palette",
        ],
        "tree" => &[
            "-L",
            "--level",
            "-P",
            "--pattern",
            "-I",
            "--ignore",
            "-o",
            "--output",
            "-H",
            "--hintro",
            "--filelimit",
            "--charset",
            "--timefmt",
            "--sort",
            "--fromfile",
        ],
        "du" => &[
            "-d",
            "--max-depth",
            "-B",
            "--block-size",
            "-t",
            "--threshold",
            "--exclude",
            "--exclude-from",
            "--time-style",
            "--files0-from",
            "-X",
        ],
        "bash" | "sh" | "zsh" => &["-c", "--command", "-o", "-O", "--rcfile", "--init-file"],
        "python" | "python3" => &["-m", "-c", "-W", "-X", "-Q", "--check-hash-based-pycs"],
        "node" => &[
            "-e",
            "--eval",
            "-p",
            "--print",
            "-r",
            "--require",
            "--loader",
            "--experimental-loader",
            "--import",
            "--input-type",
            "--conditions",
            "--inspect-port",
        ],
        "ruby" => &[
            "-e",
            "--eval",
            "-r",
            "--require",
            "-I",
            "--include",
            "-C",
            "--directory",
            "-E",
            "--encoding",
            "--external-encoding",
            "--internal-encoding",
        ],
        "perl" => &["-e", "-E", "-M", "-m", "-I"],
        _ => &[],
    }
}

fn option_takes_value(command: &str, option: &str) -> bool {
    if option.contains('=') {
        return false;
    }
    option_matches(value_options(command), option)
}

/// Options after which the remaining words are command or module arguments,
/// not paths that must exist.
fn option_ends_path_check(command: &str, option: &str) -> bool {
    let options = match command {
        "bash" | "sh" | "zsh" => &["-c", "--command"][..],
        "python" | "python3" => &["-c", "-m"][..],
        "node" => &["-e", "--eval", "-p", "--print"][..],
        "ruby" => &["-e", "--eval"][..],
        "perl" => &["-e", "-E"][..],
        "open" => &["--args"][..],
        _ => &[][..],
    };
    option_matches(options, option)
}

/// Returns whether a short-option cluster contains `flag` (`-fr`, `-rf`).
fn short_flag(option: &str, flag: char) -> bool {
    option.starts_with('-')
        && !option.starts_with("--")
        && option[1..].chars().any(|character| character == flag)
}

/// Matches an option token against a spelling list, ignoring an attached value
/// after `=` and accepting a short flag in a cluster by its last character.
fn option_matches(options: &[&str], option: &str) -> bool {
    if option == "-" || option == "--" {
        return false;
    }
    if let Some(long) = option.strip_prefix("--") {
        let name = long.split('=').next().unwrap_or(long);
        return options
            .iter()
            .any(|candidate| candidate.strip_prefix("--") == Some(name));
    }
    if !option.starts_with('-') {
        return false;
    }
    let Some(last) = option.chars().last() else {
        return false;
    };
    options.iter().any(|candidate| {
        candidate.len() == 2 && candidate.starts_with('-') && candidate.chars().nth(1) == Some(last)
    })
}

fn resolve_path(word: &str, cwd: &str) -> std::path::PathBuf {
    let trimmed = word.trim_end_matches('/');
    let trimmed = if trimmed.is_empty() { "/" } else { trimmed };
    if trimmed == "~" {
        return resolve_directory("~/", cwd);
    }
    match trimmed.rfind('/') {
        Some(slash) => resolve_directory(&trimmed[..=slash], cwd).join(&trimmed[slash + 1..]),
        None => Path::new(cwd).join(trimmed),
    }
}

/// Splits `command` into backslash-unescaped words with their byte offsets,
/// stopping at the first word containing shell syntax that is not modelled.
fn shell_words(command: &str) -> Vec<(usize, String)> {
    let mut words = Vec::new();
    let mut start = None;
    let mut escaped = false;
    let mut boundaries = Vec::new();
    for (index, character) in command.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            start.get_or_insert(index);
        } else if character.is_whitespace() {
            if let Some(word_start) = start.take() {
                boundaries.push((word_start, index));
            }
        } else {
            start.get_or_insert(index);
        }
    }
    if let Some(word_start) = start {
        boundaries.push((word_start, command.len()));
    }
    for (word_start, word_end) in boundaries {
        match unescape_path_token(&command[word_start..word_end]) {
            Some(word) => words.push((word_start, word)),
            None => break,
        }
    }
    words
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
    fn drops_history_paths_missing_from_the_working_directory() {
        let directory = tempfile::tempdir().unwrap();
        let cwd = directory.path().to_str().unwrap();
        std::fs::create_dir(directory.path().join("src")).unwrap();
        std::fs::write(directory.path().join("src/main.rs"), "").unwrap();
        std::fs::write(directory.path().join("notes.md"), "").unwrap();
        std::fs::write(directory.path().join("run.sh"), "").unwrap();

        let store = Store::in_memory().unwrap();
        for command in [
            "cat src/main.rs",
            "cat src/gone.rs",
            "cat other/file.rs",
            "cat notes.md",
            "cat missing.md",
            "cd src",
            "cd notes.md",
            "cd elsewhere",
            "rmdir notes.md",
            "./run.sh --fast",
            "./absent.sh",
            "git checkout feature/login",
            "git log -- lib/old.rs",
            "ssh host.example.com",
            "echo \"$HOME/nope\"",
        ] {
            store.record(command, "/other", 0, 1, "test", true).unwrap();
        }
        let offered = |prefix: &str| -> Vec<String> {
            candidates(&store, prefix, cwd, 32, true)
                .unwrap()
                .into_iter()
                .map(|candidate| candidate.display)
                .collect()
        };

        let mut cat = offered("cat ");
        cat.sort();
        assert_eq!(cat, ["cat notes.md", "cat src/main.rs"]);
        assert_eq!(offered("cd "), ["cd src"]);
        assert!(offered("rmdir ").is_empty());
        assert_eq!(offered("./"), ["./run.sh --fast"]);
        assert_eq!(offered("git "), ["git checkout feature/login"]);
        assert_eq!(offered("ssh "), ["ssh host.example.com"]);
        assert_eq!(offered("echo "), ["echo \"$HOME/nope\""]);
        // Words the user already typed are theirs; only the completion is judged.
        assert!(offered("cat src/go").is_empty());
    }

    #[test]
    fn keeps_path_history_with_wrappers_and_option_values() {
        let directory = tempfile::tempdir().unwrap();
        let cwd = directory.path().to_str().unwrap();
        std::fs::create_dir(directory.path().join("src")).unwrap();
        std::fs::write(directory.path().join("src/main.rs"), "").unwrap();
        std::fs::write(directory.path().join("run.sh"), "").unwrap();
        std::fs::create_dir(directory.path().join(".venv")).unwrap();

        let store = Store::in_memory().unwrap();
        for command in [
            "sudo cat src/main.rs",
            "sudo python -m venv .venv2",
            "command cat src/main.rs",
            "head -n 10 src/main.rs",
            "tail --lines 20 src/main.rs",
            "less -M src/main.rs",
            "less -M missing.rs",
            "less --status-column src/main.rs",
            "less --status-column missing.rs",
            "chmod +x run.sh",
            "chmod 755 run.sh",
            "chown user run.sh",
            "python -m venv .venv",
            "python -m pip install -r requirements.txt",
            "stat -c %s src/main.rs",
            "tree -L 2 .",
            "bash -c echo hello",
            "vim src/new.rs",
            "code src/new.rs",
            "mkdir src/generated",
            "rm -f missing.log",
            "rm -fr missing-dir",
            "rm -rf missing-other",
            "rm src/gone.rs",
        ] {
            store.record(command, "/other", 0, 1, "test", true).unwrap();
        }
        let offered = |prefix: &str| -> Vec<String> {
            candidates(&store, prefix, cwd, 32, true)
                .unwrap()
                .into_iter()
                .map(|candidate| candidate.display)
                .collect()
        };

        assert_eq!(
            offered("sudo "),
            ["sudo cat src/main.rs", "sudo python -m venv .venv2",],
            "wrapper target was treated as a path argument"
        );
        assert_eq!(offered("sudo c"), ["sudo cat src/main.rs"]);
        assert_eq!(offered("command "), ["command cat src/main.rs"]);
        assert_eq!(
            offered("head "),
            ["head -n 10 src/main.rs"],
            "option value was treated as a path"
        );
        assert_eq!(offered("tail "), ["tail --lines 20 src/main.rs"]);
        assert_eq!(
            offered("less "),
            ["less --status-column src/main.rs", "less -M src/main.rs",],
            "less -M and --status-column are flags, not value options"
        );
        assert_eq!(
            offered("chmod "),
            ["chmod +x run.sh", "chmod 755 run.sh"],
            "chmod mode was treated as a path"
        );
        assert_eq!(offered("chown "), ["chown user run.sh"]);
        assert_eq!(
            offered("python "),
            [
                "python -m pip install -r requirements.txt",
                "python -m venv .venv",
            ]
        );
        assert_eq!(offered("stat "), ["stat -c %s src/main.rs"]);
        assert_eq!(offered("tree "), ["tree -L 2 ."]);
        assert_eq!(offered("bash "), ["bash -c echo hello"]);
        assert_eq!(offered("vim "), ["vim src/new.rs"]);
        assert_eq!(offered("code "), ["code src/new.rs"]);
        assert_eq!(offered("mkdir "), ["mkdir src/generated"]);
        assert_eq!(
            offered("rm "),
            [
                "rm -f missing.log",
                "rm -fr missing-dir",
                "rm -rf missing-other",
            ]
        );
    }

    #[test]
    fn drops_missing_file_paths_recorded_in_the_same_directory() {
        let directory = tempfile::tempdir().unwrap();
        let cwd = directory.path().to_str().unwrap();
        let store = Store::in_memory().unwrap();
        store
            .record("git log docs/old.md", cwd, 0, 1, "test", true)
            .unwrap();
        store
            .record("git checkout feature/login", cwd, 0, 2, "test", true)
            .unwrap();

        let offered: Vec<String> = candidates(&store, "git ", cwd, 32, true)
            .unwrap()
            .into_iter()
            .map(|candidate| candidate.display)
            .collect();
        assert_eq!(offered, ["git checkout feature/login"]);
    }

    #[test]
    fn script_arguments_are_not_paths() {
        let directory = tempfile::tempdir().unwrap();
        let cwd = directory.path().to_str().unwrap();
        std::fs::write(directory.path().join("manage.py"), "").unwrap();
        std::fs::write(directory.path().join("deploy.sh"), "").unwrap();
        let store = Store::in_memory().unwrap();
        for command in [
            "python manage.py migrate",
            "python missing.py arg",
            "bash deploy.sh production",
            "bash missing.sh arg",
        ] {
            store.record(command, cwd, 0, 1, "test", true).unwrap();
        }
        let offered = |prefix: &str| -> Vec<String> {
            candidates(&store, prefix, cwd, 32, true)
                .unwrap()
                .into_iter()
                .map(|candidate| candidate.display)
                .collect()
        };

        assert_eq!(offered("python "), ["python manage.py migrate"]);
        assert_eq!(offered("bash "), ["bash deploy.sh production"]);
    }

    #[test]
    fn rejects_mkdir_of_existing_files() {
        let directory = tempfile::tempdir().unwrap();
        let cwd = directory.path().to_str().unwrap();
        std::fs::write(directory.path().join("README.md"), "").unwrap();
        let store = Store::in_memory().unwrap();
        store
            .record("mkdir README.md", cwd, 1, 2, "test", true)
            .unwrap();
        store
            .record("mkdir generated", cwd, 0, 1, "test", true)
            .unwrap();

        let offered: Vec<String> = candidates(&store, "mkdir ", cwd, 32, true)
            .unwrap()
            .into_iter()
            .map(|candidate| candidate.display)
            .collect();
        assert_eq!(offered, ["mkdir generated"]);
    }

    #[test]
    fn keeps_history_with_trailing_comments() {
        let directory = tempfile::tempdir().unwrap();
        let cwd = directory.path().to_str().unwrap();
        std::fs::write(directory.path().join("notes.md"), "").unwrap();
        let store = Store::in_memory().unwrap();
        store
            .record("cat notes.md # reviewed", cwd, 0, 1, "test", true)
            .unwrap();

        let offered: Vec<String> = candidates(&store, "cat ", cwd, 32, true)
            .unwrap()
            .into_iter()
            .map(|candidate| candidate.display)
            .collect();
        assert_eq!(offered, ["cat notes.md # reviewed"]);
    }

    #[test]
    fn large_limits_do_not_panic() {
        let directory = tempfile::tempdir().unwrap();
        let cwd = directory.path().to_str().unwrap();
        let store = Store::in_memory().unwrap();
        store.record("git status", cwd, 0, 1, "test", true).unwrap();

        let candidates = candidates(&store, "git ", cwd, 300, true).unwrap();
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn resolves_home_directory_with_a_trailing_slash() {
        let directory = tempfile::tempdir().unwrap();
        let cwd = directory.path().to_str().unwrap();
        assert_eq!(resolve_path("~/", cwd), resolve_directory("~/", cwd));
        assert_eq!(resolve_path("~", cwd), resolve_directory("~/", cwd));
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
