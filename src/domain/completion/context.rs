//! Conservative recognition of root-option, subcommand, and option-value
//! contexts. Each parser returns `None` for anything involving shell syntax.

use crate::domain::valid_command_prefix;

pub(crate) fn option_context(buffer: &str) -> Option<(&str, &str)> {
    if !safe_structured_buffer(buffer) {
        return None;
    }
    let argument_start = buffer.rfind(char::is_whitespace)? + 1;
    let prefix = &buffer[argument_start..];
    if !prefix.starts_with('-') {
        return None;
    }
    let mut words = buffer[..argument_start].split_ascii_whitespace();
    let command = words.next()?;
    if !valid_command_prefix(command) || words.any(|word| !word.starts_with('-') || word == "--") {
        return None;
    }
    Some((command, prefix))
}

pub(crate) fn subcommand_context(buffer: &str) -> Option<(&str, &str)> {
    if !safe_structured_buffer(buffer) {
        return None;
    }
    let argument_start = buffer.rfind(char::is_whitespace)? + 1;
    let prefix = &buffer[argument_start..];
    if prefix.starts_with('-') || !valid_structured_prefix(prefix) {
        return None;
    }
    let mut words = buffer[..argument_start].split_ascii_whitespace();
    let command = words.next()?;
    if !valid_command_prefix(command) || words.next().is_some() {
        return None;
    }
    Some((command, prefix))
}

pub(crate) fn value_context(buffer: &str) -> Option<(&str, &str, &str)> {
    if !safe_structured_buffer(buffer) {
        return None;
    }
    let argument_start = buffer.rfind(char::is_whitespace)? + 1;
    let token = &buffer[argument_start..];
    let mut words = buffer[..argument_start].split_ascii_whitespace();
    let command = words.next()?;
    if !valid_command_prefix(command) {
        return None;
    }

    if let Some((option, prefix)) = token.split_once('=') {
        if words.next().is_none() && valid_option_context_spelling(option) {
            return Some((command, option, prefix));
        }
        return None;
    }

    let option = words.next()?;
    if words.next().is_some() || token.starts_with('-') || !valid_option_context_spelling(option) {
        return None;
    }
    Some((command, option, token))
}

fn safe_structured_buffer(buffer: &str) -> bool {
    buffer.is_ascii()
        && !buffer
            .chars()
            .any(|character| character.is_control() || "'\"\\;&|><$`(){}[]!*?".contains(character))
}

fn valid_structured_prefix(prefix: &str) -> bool {
    prefix.is_empty()
        || (prefix.as_bytes()[0].is_ascii_alphanumeric()
            && prefix.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+')
            }))
}

fn valid_option_context_spelling(option: &str) -> bool {
    option != "--"
        && option.starts_with('-')
        && option
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_safe_root_option_contexts() {
        assert_eq!(option_context("git --ver"), Some(("git", "--ver")));
        assert_eq!(option_context("git status --short"), None);
        assert_eq!(
            option_context("git --quiet --short"),
            Some(("git", "--short"))
        );
        assert_eq!(option_context("git -- --literal"), None);
        assert_eq!(option_context("git \"--ver"), None);
        assert_eq!(option_context("--ver"), None);
    }

    #[test]
    fn recognizes_only_unambiguous_structured_contexts() {
        assert_eq!(subcommand_context("tool bu"), Some(("tool", "bu")));
        assert_eq!(subcommand_context("tool "), Some(("tool", "")));
        assert_eq!(subcommand_context("tool other bu"), None);
        assert_eq!(subcommand_context("tool --flag "), None);
        assert_eq!(
            value_context("tool --color=a"),
            Some(("tool", "--color", "a"))
        );
        assert_eq!(
            value_context("tool --color a"),
            Some(("tool", "--color", "a"))
        );
        assert_eq!(value_context("tool other --color a"), None);
    }
}
