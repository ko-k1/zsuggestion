//! Bounded local filesystem candidates for argument positions and explicit
//! paths in command position, plus merging them behind higher tiers.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use super::next_segment;
use crate::transport::protocol::{Candidate, CandidateKind, CandidateSource, CompletionResponse};

const MAX_DIRECTORY_ENTRIES: usize = 1024;

pub fn filesystem_candidates(
    buffer: &str,
    cursor_byte: usize,
    cwd: &str,
) -> Result<Vec<Candidate>> {
    if cursor_byte != buffer.len() || !buffer.is_char_boundary(cursor_byte) {
        return Ok(Vec::new());
    }
    // Argument positions accept any path. The command position accepts only
    // explicit paths (`./run`, `../bin/x`, `/usr/bin/x`, `~/bin/x`), so plain
    // command names never turn into filesystem guesses.
    let argument_start = current_argument_start(buffer);
    let command_position = argument_start.is_none();
    let Some(token) = unescape_path_token(&buffer[argument_start.unwrap_or(0)..]) else {
        return Ok(Vec::new());
    };
    if token.starts_with('-') || (command_position && !explicit_path_token(&token)) {
        return Ok(Vec::new());
    }

    let (directory_text, name_prefix) = token
        .rfind('/')
        .map_or(("", token.as_str()), |slash| token.split_at(slash + 1));
    let directory = resolve_directory(directory_text, cwd);
    let Ok(children) = fs::read_dir(&directory) else {
        return Ok(Vec::new());
    };
    let show_hidden = name_prefix.starts_with('.');
    let mut matches = Vec::new();
    for child in children.flatten() {
        let Some(name) = child.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name.is_empty()
            || (!show_hidden && name.starts_with('.'))
            || !name.starts_with(name_prefix)
            || name.chars().any(char::is_control)
        {
            continue;
        }
        let Ok(file_type) = child.file_type() else {
            continue;
        };
        if !(file_type.is_dir() || file_type.is_file() || file_type.is_symlink()) {
            continue;
        }
        let is_directory = file_type.is_dir() || (file_type.is_symlink() && child.path().is_dir());
        let executable = command_position && !is_directory && is_executable(&child.path());
        let suffix = &name[name_prefix.len()..];
        let mut insert_text = escape_path_suffix(suffix);
        if token.is_empty() && name.starts_with('-') {
            insert_text.insert_str(0, "./");
        }
        if is_directory {
            insert_text.push('/');
        }
        let exact_file = !is_directory && insert_text.is_empty();
        if exact_file {
            insert_text.push(' ');
        }
        matches.push((is_directory, executable, name, insert_text, exact_file));
        if matches.len() >= MAX_DIRECTORY_ENTRIES {
            break;
        }
    }
    matches.sort_unstable_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| right.1.cmp(&left.1))
            .then_with(|| left.2.cmp(&right.2))
    });
    Ok(matches
        .into_iter()
        .map(
            |(is_directory, executable, _, insert_text, exact_file)| Candidate {
                display: if exact_file {
                    buffer.to_owned()
                } else {
                    format!("{buffer}{insert_text}")
                },
                description: if is_directory {
                    "Directory"
                } else if executable {
                    "Executable"
                } else {
                    "File"
                }
                .to_owned(),
                description_pending: false,
                kind: if is_directory {
                    CandidateKind::Directory
                } else {
                    CandidateKind::File
                },
                accept_text: next_segment(&insert_text),
                insert_text,
                source: CandidateSource::Filesystem,
            },
        )
        .collect())
}

pub fn merge_filesystem_candidates(
    response: &mut CompletionResponse,
    paths: Vec<Candidate>,
    limit: usize,
) {
    if paths.is_empty() || limit == 0 {
        return;
    }
    let history_count = response
        .candidates
        .iter()
        .rev()
        .take_while(|candidate| candidate.source == CandidateSource::History)
        .count();
    let mut original = std::mem::take(&mut response.candidates);
    let history = original.split_off(original.len() - history_count);
    let mut seen = HashSet::new();
    for candidate in original {
        if response.candidates.len() >= limit {
            break;
        }
        if seen.insert(candidate.display.clone()) {
            response.candidates.push(candidate);
        }
    }
    for path in paths {
        if response.candidates.len() >= limit {
            break;
        }
        if seen.insert(path.display.clone()) {
            response.candidates.push(path);
        }
    }
    for candidate in history {
        if response.candidates.len() >= limit {
            break;
        }
        if seen.insert(candidate.display.clone()) {
            response.candidates.push(candidate);
        }
    }
}

fn explicit_path_token(token: &str) -> bool {
    ["./", "../", "/", "~/"]
        .iter()
        .any(|prefix| token.starts_with(prefix))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    false
}

fn resolve_directory(directory_text: &str, cwd: &str) -> PathBuf {
    if directory_text == "~/" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(cwd));
    }
    if let Some(relative) = directory_text.strip_prefix("~/") {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(cwd))
            .join(relative);
    }
    let directory = Path::new(directory_text);
    if directory.is_absolute() {
        directory.to_owned()
    } else {
        Path::new(cwd).join(directory)
    }
}

fn unescape_path_token(value: &str) -> Option<String> {
    let mut unescaped = String::new();
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            unescaped.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character.is_control() || "'\";&|><$`(){}[]!*?".contains(character) {
            return None;
        } else {
            unescaped.push(character);
        }
    }
    (!escaped).then_some(unescaped)
}

fn current_argument_start(buffer: &str) -> Option<usize> {
    let mut start = None;
    let mut escaped = false;
    for (index, character) in buffer.char_indices() {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character.is_whitespace() {
            start = Some(index + character.len_utf8());
        }
    }
    start
}

fn escape_path_suffix(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        if character.is_ascii()
            && !(character.is_ascii_alphanumeric() || "_-+.@%,".contains(character))
        {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn completes_filesystem_entries_at_argument_positions() {
        let directory = tempdir().unwrap();
        fs::create_dir(directory.path().join("alpha-dir")).unwrap();
        fs::write(directory.path().join("alpha-file"), "file").unwrap();
        fs::write(directory.path().join("alpha space"), "file").unwrap();
        fs::write(directory.path().join(".hidden"), "file").unwrap();
        fs::write(directory.path().join("-rf"), "file").unwrap();
        fs::write(directory.path().join("=command"), "file").unwrap();
        fs::create_dir(directory.path().join("space dir")).unwrap();
        fs::write(directory.path().join("space dir/child"), "file").unwrap();

        let buffer = "scp -r alpha";
        let candidates =
            filesystem_candidates(buffer, buffer.len(), directory.path().to_str().unwrap())
                .unwrap();
        assert_eq!(candidates[0].display, "scp -r alpha-dir/");
        assert_eq!(candidates[0].kind, CandidateKind::Directory);
        assert!(candidates.iter().any(|candidate| {
            candidate.display == "scp -r alpha-file" && candidate.kind == CandidateKind::File
        }));
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.display == "scp -r alpha\\ space")
        );
        assert!(
            !candidates
                .iter()
                .any(|candidate| candidate.display.contains(".hidden"))
        );

        let nested = "scp -r space\\ dir/ch";
        let candidates =
            filesystem_candidates(nested, nested.len(), directory.path().to_str().unwrap())
                .unwrap();
        assert_eq!(candidates[0].display, "scp -r space\\ dir/child");

        let exact = "scp -r alpha-file";
        let candidates =
            filesystem_candidates(exact, exact.len(), directory.path().to_str().unwrap()).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].display, exact);
        assert_eq!(candidates[0].accept_text, " ");

        let blank = "scp -r ";
        let candidates =
            filesystem_candidates(blank, blank.len(), directory.path().to_str().unwrap()).unwrap();
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.display == "scp -r ./-rf")
        );
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.display == "scp -r \\=command")
        );
    }

    #[test]
    fn lists_paths_after_an_empty_argument_and_hides_them_in_command_position() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("visible"), "file").unwrap();

        let buffer = "command ";
        let candidates =
            filesystem_candidates(buffer, buffer.len(), directory.path().to_str().unwrap())
                .unwrap();
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.display == "command visible")
        );
        assert!(
            filesystem_candidates("com", 3, directory.path().to_str().unwrap())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn completes_explicit_paths_in_command_position() {
        let directory = tempdir().unwrap();
        let cwd = directory.path().to_str().unwrap();
        fs::create_dir(directory.path().join("build")).unwrap();
        fs::write(directory.path().join("notes.txt"), "file").unwrap();
        fs::write(directory.path().join("run.sh"), "#!/bin/sh").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                directory.path().join("run.sh"),
                fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
        fs::write(directory.path().join("build/app"), "file").unwrap();

        let candidates = filesystem_candidates("./", 2, cwd).unwrap();
        let displays: Vec<_> = candidates.iter().map(|c| c.display.as_str()).collect();
        assert_eq!(displays, ["./build/", "./run.sh", "./notes.txt"]);
        assert_eq!(candidates[0].kind, CandidateKind::Directory);
        #[cfg(unix)]
        assert_eq!(candidates[1].description, "Executable");
        assert_eq!(candidates[2].description, "File");

        let candidates = filesystem_candidates("./r", 3, cwd).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].display, "./run.sh");

        let candidates = filesystem_candidates("./build/", 8, cwd).unwrap();
        assert_eq!(candidates[0].display, "./build/app");

        let nested = directory.path().join("build");
        let candidates = filesystem_candidates("../", 3, nested.to_str().unwrap()).unwrap();
        assert!(candidates.iter().any(|c| c.display == "../run.sh"));

        // Bare words and `.`/`..` alone stay command or history territory.
        for buffer in ["run", ".", "..", "build/"] {
            assert!(
                filesystem_candidates(buffer, buffer.len(), cwd)
                    .unwrap()
                    .is_empty(),
                "{buffer}"
            );
        }
    }

    #[test]
    fn filesystem_reserves_capacity_behind_command_suggestions() {
        let mut response = CompletionResponse {
            replace_start_byte: 5,
            replace_end_byte: 5,
            candidates: (0..4)
                .map(|index| Candidate {
                    display: format!("cmd entry-{index}"),
                    description: String::new(),
                    description_pending: false,
                    kind: CandidateKind::Command,
                    insert_text: index.to_string(),
                    accept_text: index.to_string(),
                    source: CandidateSource::Command,
                })
                .collect(),
            enrichment_pending: false,
        };
        let paths = (0..2)
            .map(|index| Candidate {
                display: format!("cmd path-{index}"),
                description: "File".to_owned(),
                description_pending: false,
                kind: CandidateKind::File,
                insert_text: index.to_string(),
                accept_text: index.to_string(),
                source: CandidateSource::Filesystem,
            })
            .collect();
        merge_filesystem_candidates(&mut response, paths, 6);
        assert_eq!(response.candidates.len(), 6);
        assert!(
            response
                .candidates
                .iter()
                .take(4)
                .all(|candidate| candidate.source == CandidateSource::Command)
        );
        assert!(
            response
                .candidates
                .iter()
                .skip(4)
                .all(|candidate| candidate.source == CandidateSource::Filesystem)
        );

        let mut response = CompletionResponse::empty(0);
        let paths = (0..2)
            .map(|index| Candidate {
                display: format!("cmd path-{index}"),
                description: "File".to_owned(),
                description_pending: false,
                kind: CandidateKind::File,
                insert_text: index.to_string(),
                accept_text: index.to_string(),
                source: CandidateSource::Filesystem,
            })
            .collect();
        merge_filesystem_candidates(&mut response, paths, 1);
        assert_eq!(response.candidates.len(), 1);
        assert!(
            response
                .candidates
                .iter()
                .all(|c| c.source == CandidateSource::Filesystem)
        );
    }
}
