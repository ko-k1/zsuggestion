//! Shared command/path classification for the filesystem and history tiers.

/// Commands whose positional arguments are paths.
pub(crate) const PATH_COMMANDS: &[&str] = &[
    "cd", "chdir", "pushd", "ls", "cat", "less", "more", "head", "tail", "bat", "rm", "rmdir",
    "open", "xdg-open", "file", "stat", "wc", "diff", "chmod", "chown", "tree", "du",
];

/// Commands whose first positional operand is a script and whose later
/// operands are the script's own arguments, not paths that must exist.
pub(crate) const SCRIPT_COMMANDS: &[&str] = &[
    "bash", "sh", "zsh", "python", "python3", "node", "ruby", "perl", "source", ".",
];

/// Commands that may legitimately name a path they are about to create.
pub(crate) const PATHS_MAY_NOT_EXIST: &[&str] = &[
    "vi", "vim", "nvim", "nano", "emacs", "code", "mkdir", "touch", "tee",
];

/// Commands whose positional arguments must name directories.
pub(crate) const DIRECTORY_ONLY_COMMANDS: &[&str] = &["cd", "chdir", "pushd", "rmdir", "mkdir"];

/// Commands that take no filesystem path arguments at all.
pub(crate) const NO_PATH_COMMANDS: &[&str] = &["popd", "dirs"];

/// Commands that run another command from their first non-option operand.
const WRAPPER_COMMANDS: &[&str] = &["sudo", "command", "exec", "nohup", "builtin", "time", "env"];

pub(crate) fn command_word(buffer: &str) -> Option<&str> {
    buffer.split_ascii_whitespace().next()
}

/// Returns the command word only when it describes a simple command line.
/// Compound commands, assignments, wrappers, and comments make the first word
/// unreliable for classifying the command at the cursor.
pub(crate) fn classification_command(buffer: &str) -> Option<&str> {
    if has_unquoted_syntax(buffer) {
        return None;
    }
    let word = command_word(buffer)?;
    if word.contains('=') || WRAPPER_COMMANDS.contains(&word) {
        return None;
    }
    Some(word)
}

/// Returns whether `command` contains unquoted shell syntax the path
/// classifier does not model. Such commands are accepted conservatively
/// instead of being judged by their first word.
pub(crate) fn has_unquoted_syntax(command: &str) -> bool {
    let mut quote = None;
    let mut escaped = false;
    for character in command.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        match quote {
            Some(quoting) => {
                if character == quoting {
                    quote = None;
                }
            }
            None => {
                if character == '\'' || character == '"' {
                    quote = Some(character);
                } else if (character.is_control() && character != '\t')
                    || ";&|<>(){}#".contains(character)
                {
                    return true;
                }
            }
        }
    }
    false
}

pub(crate) fn is_path_command(command: &str) -> bool {
    PATH_COMMANDS.contains(&command)
        || SCRIPT_COMMANDS.contains(&command)
        || PATHS_MAY_NOT_EXIST.contains(&command)
}

pub(crate) fn is_script_command(command: &str) -> bool {
    SCRIPT_COMMANDS.contains(&command)
}

pub(crate) fn paths_may_not_exist(command: &str) -> bool {
    PATHS_MAY_NOT_EXIST.contains(&command)
}

pub(crate) fn takes_directories_only(command: &str) -> bool {
    DIRECTORY_ONLY_COMMANDS.contains(&command)
}

pub(crate) fn takes_no_paths(command: &str) -> bool {
    NO_PATH_COMMANDS.contains(&command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_only_simple_command_lines() {
        assert_eq!(classification_command("cd "), Some("cd"));
        assert_eq!(classification_command("cd\t"), Some("cd"));
        assert_eq!(classification_command("cd 'space dir' "), Some("cd"));
        assert_eq!(classification_command("cd foo\\#bar/ "), Some("cd"));
        assert_eq!(classification_command("FOO=1 cd "), None);
        assert_eq!(classification_command("sudo cd "), None);
        assert_eq!(classification_command("cd .. && ls "), None);
        assert_eq!(classification_command("cat notes.md # reviewed"), None);
    }

    #[test]
    fn ignores_syntax_inside_quotes_and_after_escapes() {
        assert!(!has_unquoted_syntax("cat 'a;b' \"c|d\""));
        assert!(!has_unquoted_syntax("cat file\\;name"));
        assert!(has_unquoted_syntax("cat file # reviewed"));
        assert!(has_unquoted_syntax("cd .. && ls"));
        assert!(has_unquoted_syntax("cd foo\nbar"));
    }
}
