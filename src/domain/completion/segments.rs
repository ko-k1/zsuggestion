//! Segment acceptance logic for history candidates.

/// Returns the next shell-word segment boundary of `suffix`.
///
/// Quoting, escaping, brackets, and semantic separators (`@`, `=`, `,`, `:`,
/// `/`) are honored so the accept text stops at a meaningful boundary rather
/// than greedily consuming the rest of the line.
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
}
