//! Conservative parsers for `man` and `--help` output.
//!
//! Every parser prefers returning nothing over guessing: lines containing
//! shell syntax, terminal controls, or ambiguous layouts are discarded.

use std::collections::HashSet;

use super::super::{
    MAX_OPTION_COUNT, MAX_OPTION_SPELLING_BYTES, MAX_SUBCOMMAND_COUNT, MAX_VALUE_COUNT,
    OptionMatch, OptionValues, SubcommandMatch, ValueMatch, valid_subcommand_name, valid_value,
};

pub(super) fn parse_man_description(name: &str, output: &str) -> Option<String> {
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

pub(super) fn parse_help_description(name: &str, output: &str) -> Option<String> {
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

pub(super) fn parse_options(output: &str) -> Vec<OptionMatch> {
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

pub(super) fn parse_subcommands(output: &str) -> Vec<SubcommandMatch> {
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

pub(super) fn parse_subcommand_declaration(line: &str) -> Option<(Vec<String>, String)> {
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

pub(super) fn parse_option_values(output: &str) -> Vec<OptionValues> {
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

pub(super) fn insert_option_values(
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

pub(super) fn documented_values(line: &str) -> (Vec<ValueMatch>, bool) {
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

pub(super) fn parse_value_list(values: &str, separator: char) -> Vec<ValueMatch> {
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

pub(super) fn documented_value_bullet(line: &str) -> Option<ValueMatch> {
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

pub(super) struct ParsedOptionDeclaration {
    spellings: Vec<String>,
    description: Option<String>,
}

pub(super) fn parse_option_declaration(line: &str) -> Option<ParsedOptionDeclaration> {
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

pub(super) fn option_spelling_at(line: &str, position: usize) -> Option<(&str, usize)> {
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

pub(super) fn valid_option_spelling(spelling: &str) -> bool {
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

pub(super) fn option_line_has_unsafe_data(line: &str) -> bool {
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

pub(super) fn is_option_argument(token: &str) -> bool {
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

pub(super) fn skip_ascii_spaces(bytes: &[u8], position: &mut usize) {
    while bytes.get(*position).is_some_and(u8::is_ascii_whitespace) {
        *position += 1;
    }
}

pub(super) fn is_options_heading(line: &str) -> bool {
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

pub(super) fn is_commands_heading(line: &str) -> bool {
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

pub(super) fn is_section_heading(line: &str) -> bool {
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

pub(super) fn clean_lines(output: &str) -> impl Iterator<Item = String> + '_ {
    output
        .lines()
        .filter_map(clean_line)
        .map(|line| line.trim().to_owned())
}

pub(super) fn clean_line(line: &str) -> Option<String> {
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

pub(super) fn sanitize_description(description: &str) -> Option<String> {
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

pub(super) fn is_directional_format(character: char) -> bool {
    matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

#[cfg(test)]
mod tests {
    use super::*;

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
