//! Bounded, sanitized previews for the inline menu.
//!
//! Previews never go through a shell: file previews read at most 16 KiB of a
//! regular, non-symlink file, and command previews only run an allowlisted
//! read-only command (`ls`, `gls`, `eza`) with a hard timeout. Output is
//! emitted as NUL-separated records for the Zsh frontend.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

/// Writes a NUL-separated summary of a local regular file.
pub fn write_file_preview(output: &mut impl Write, path: PathBuf, cwd: &Path) -> Result<()> {
    let path = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    if is_env_file(&path) {
        bail!("preview disabled for .env files");
    }
    let metadata = fs::symlink_metadata(&path)
        .with_context(|| format!("failed to inspect preview target {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("preview target is not a regular non-symlink file");
    }
    let mut bytes = Vec::new();
    File::open(&path)?.take(16 * 1024).read_to_end(&mut bytes)?;
    let kind = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "PNG image"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "JPEG image"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "GIF image"
    } else if bytes.starts_with(b"%PDF-") {
        "PDF document"
    } else if bytes.contains(&0) {
        "binary file"
    } else {
        "text file"
    };
    for line in [
        format!("Type: {kind}"),
        format!("Size: {} bytes", metadata.len()),
    ] {
        output.write_all(line.as_bytes())?;
        output.write_all(&[0])?;
    }
    if kind == "text file" {
        let text = String::from_utf8_lossy(&bytes);
        for source in text.lines().take(8) {
            let line: String = source
                .chars()
                .take(160)
                .map(|character| {
                    if character == '\t' {
                        ' '
                    } else if character.is_ascii() && !character.is_control() {
                        character
                    } else {
                        '?'
                    }
                })
                .collect();
            output.write_all(line.as_bytes())?;
            output.write_all(&[0])?;
        }
    } else if matches!(
        kind,
        "PNG image" | "JPEG image" | "GIF image" | "PDF document"
    ) {
        output.write_all(b"Graphical preview requires the future PTY frontend")?;
        output.write_all(&[0])?;
    }
    Ok(())
}

fn is_env_file(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.as_encoded_bytes().starts_with(b".env"))
}

/// Runs an allowlisted listing command and writes NUL-separated `text`,
/// `zle-styles` record pairs.
pub fn write_command_preview(output: &mut impl Write, line: &str, cwd: &Path) -> Result<()> {
    let lines = command_preview_lines(line, cwd)?;
    for line in lines {
        output.write_all(line.text.as_bytes())?;
        output.write_all(&[0])?;
        output.write_all(line.styles.as_bytes())?;
        output.write_all(&[0])?;
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct CommandPreviewLine {
    text: String,
    styles: String,
}

#[derive(Clone, Default, PartialEq, Eq)]
struct AnsiStyle {
    foreground: Option<String>,
    background: Option<String>,
    bold: bool,
    underline: bool,
    standout: bool,
}

impl AnsiStyle {
    fn zle(&self) -> String {
        let mut values = Vec::new();
        if let Some(foreground) = &self.foreground {
            values.push(format!("fg={foreground}"));
        }
        if let Some(background) = &self.background {
            values.push(format!("bg={background}"));
        }
        if self.bold {
            values.push("bold".to_owned());
        }
        if self.underline {
            values.push("underline".to_owned());
        }
        if self.standout {
            values.push("standout".to_owned());
        }
        values.join(",")
    }
}

fn command_preview_lines(line: &str, cwd: &Path) -> Result<Vec<CommandPreviewLine>> {
    let (executable, arguments) = command_preview_argv(line)?;

    let mut command = Command::new(executable);
    command
        .args(&arguments)
        .current_dir(cwd)
        .env("LC_ALL", "C")
        .env("COLUMNS", "80")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    if executable == "eza" {
        command.env("TERM", "xterm-256color").env_remove("NO_COLOR");
    } else {
        command
            .env("TERM", "dumb")
            .env("NO_COLOR", "1")
            .env("CLICOLOR", "0");
    }
    let mut child = command.spawn().context("failed to start command preview")?;
    let stdout = child.stdout.take().expect("preview stdout is piped");
    let stderr = child.stderr.take().expect("preview stderr is piped");
    let stdout_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.take(32 * 1024).read_to_end(&mut bytes);
        bytes
    });
    let stderr_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.take(8 * 1024).read_to_end(&mut bytes);
        bytes
    });
    let deadline = Instant::now() + Duration::from_millis(600);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) | Err(_) => {
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.kill();
                break;
            }
        }
    }
    let _ = child.wait();
    let stdout = stdout_reader
        .join()
        .expect("preview stdout reader panicked");
    let stderr = stderr_reader
        .join()
        .expect("preview stderr reader panicked");
    let bytes = if stdout.is_empty() { stderr } else { stdout };
    let text = String::from_utf8_lossy(&bytes);
    Ok(text
        .lines()
        .map(parse_ansi_preview_line)
        .filter(|line| !line.text.is_empty())
        .take(8)
        .collect())
}

fn command_preview_argv(line: &str) -> Result<(&str, Vec<String>)> {
    if line.is_empty()
        || line.len() > 16 * 1024
        || line
            .chars()
            .any(|character| character.is_control() || "'\"\\;&|><$`(){}[]!*?~".contains(character))
    {
        bail!("command preview contains unsupported shell syntax");
    }
    let mut words = line.split_ascii_whitespace();
    let executable = words.next().context("command preview is empty")?;
    if !matches!(executable, "ls" | "gls" | "eza") {
        bail!("command preview only supports ls, gls, and eza");
    }
    let mut arguments = Vec::new();
    for argument in words {
        if argument.starts_with("--color")
            || argument.starts_with("--colour")
            || argument.starts_with("--hyperlink")
            || (executable == "eza" && argument.starts_with("--icons"))
            || (executable != "eza" && matches!(argument, "--dired" | "-G"))
        {
            continue;
        }
        arguments.push(argument.to_owned());
    }
    if executable == "eza" {
        arguments.push("--color=always".to_owned());
        arguments.push("--icons=never".to_owned());
        arguments.push("--width=80".to_owned());
    } else if executable == "gls" || cfg!(target_os = "linux") {
        // GNU ls only emits hyperlinks when asked (user flags are stripped
        // above), and uutils ls rejects --hyperlink entirely.
        arguments.push("--color=never".to_owned());
    }
    Ok((executable, arguments))
}

fn parse_ansi_preview_line(source: &str) -> CommandPreviewLine {
    let characters: Vec<char> = source.chars().collect();
    let mut output = String::new();
    let mut spans = Vec::new();
    let mut style = AnsiStyle::default();
    let mut run_style = String::new();
    let mut run_start = 0;
    let mut output_length = 0;
    let mut index = 0;

    while index < characters.len() && output_length < 160 {
        if characters[index] == '\x1b' {
            if characters.get(index + 1) == Some(&'[') {
                let mut end = index + 2;
                while end < characters.len() && !characters[end].is_ascii_alphabetic() {
                    end += 1;
                }
                if end < characters.len() {
                    if characters[end] == 'm' {
                        let parameters: String = characters[index + 2..end].iter().collect();
                        apply_sgr(&mut style, &parameters);
                        let next_style = style.zle();
                        if next_style != run_style {
                            push_style_span(&mut spans, run_start, output_length, &run_style);
                            run_start = output_length;
                            run_style = next_style;
                        }
                    }
                    index = end + 1;
                    continue;
                }
            } else if characters.get(index + 1) == Some(&']') {
                index += 2;
                while index < characters.len() {
                    if characters[index] == '\x07' {
                        index += 1;
                        break;
                    }
                    if characters[index] == '\x1b' && characters.get(index + 1) == Some(&'\\') {
                        index += 2;
                        break;
                    }
                    index += 1;
                }
                continue;
            }
            index += usize::from(characters.get(index + 1).is_some()) + 1;
            continue;
        }

        let character = characters[index];
        if character == '\t' {
            output.push(' ');
            output_length += 1;
        } else if character.is_ascii() && !character.is_control() {
            output.push(character);
            output_length += 1;
        } else if !character.is_control() {
            output.push('?');
            output_length += 1;
        }
        index += 1;
    }
    push_style_span(&mut spans, run_start, output_length, &run_style);
    CommandPreviewLine {
        text: output,
        styles: spans.join(";"),
    }
}

fn push_style_span(spans: &mut Vec<String>, start: usize, end: usize, style: &str) {
    if start < end && !style.is_empty() {
        spans.push(format!("{start}:{end}:{style}"));
    }
}

fn apply_sgr(style: &mut AnsiStyle, parameters: &str) {
    let values: Vec<u16> = if parameters.is_empty() {
        vec![0]
    } else {
        parameters
            .split(';')
            .map(|value| value.parse().unwrap_or(0))
            .collect()
    };
    let mut index = 0;
    while index < values.len() {
        let value = values[index];
        match value {
            0 => *style = AnsiStyle::default(),
            1 => style.bold = true,
            4 => style.underline = true,
            7 => style.standout = true,
            22 => style.bold = false,
            24 => style.underline = false,
            27 => style.standout = false,
            30..=37 => style.foreground = Some((value - 30).to_string()),
            39 => style.foreground = None,
            40..=47 => style.background = Some((value - 40).to_string()),
            49 => style.background = None,
            90..=97 => style.foreground = Some((value - 82).to_string()),
            100..=107 => style.background = Some((value - 92).to_string()),
            38 | 48 => {
                let color = if values.get(index + 1) == Some(&5) {
                    index += 2;
                    values
                        .get(index)
                        .filter(|value| **value <= 255)
                        .map(u16::to_string)
                } else if values.get(index + 1) == Some(&2) && index + 4 < values.len() {
                    let red = values[index + 2].min(255);
                    let green = values[index + 3].min(255);
                    let blue = values[index + 4].min(255);
                    index += 4;
                    Some(format!("#{red:02x}{green:02x}{blue:02x}"))
                } else {
                    None
                };
                if value == 38 {
                    style.foreground = color;
                } else {
                    style.background = color;
                }
            }
            _ => {}
        }
        index += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn suppresses_env_file_previews() {
        let directory = tempdir().unwrap();
        for name in [".env", ".env.local", ".environment"] {
            let path = directory.path().join(name);
            fs::write(&path, "SECRET=value\n").unwrap();
            assert!(is_env_file(&path));
            assert!(write_file_preview(&mut Vec::new(), path, directory.path()).is_err());
        }
        assert!(!is_env_file(&directory.path().join("example.env")));
    }

    #[test]
    fn previews_text_files_as_nul_separated_records() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("notes.txt"), "first\tline\nsecond\n").unwrap();
        let mut output = Vec::new();
        write_file_preview(&mut output, "notes.txt".into(), directory.path()).unwrap();
        assert_eq!(
            output,
            b"Type: text file\0Size: 18 bytes\0first line\0second\0".to_vec()
        );
    }

    #[test]
    fn previews_ls_without_a_shell() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("alpha.txt"), "alpha").unwrap();
        fs::write(directory.path().join("beta.txt"), "beta").unwrap();

        let lines = command_preview_lines("ls", directory.path()).unwrap();
        let preview = lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(preview.contains("alpha.txt"));
        assert!(preview.contains("beta.txt"));
    }

    #[test]
    fn command_preview_rejects_shell_syntax_and_other_commands() {
        let directory = tempdir().unwrap();
        assert!(command_preview_lines("ls; rm -rf /", directory.path()).is_err());
        assert!(command_preview_lines("echo hello", directory.path()).is_err());
        assert!(command_preview_lines("/bin/ls", directory.path()).is_err());
    }

    #[test]
    fn eza_preview_forces_colored_bounded_output() {
        let (executable, arguments) = command_preview_argv(
            "eza -l -G --icons=always --colour=always --hyperlink preview-target",
        )
        .unwrap();
        assert_eq!(executable, "eza");
        assert!(arguments.contains(&"-l".to_owned()));
        assert!(arguments.contains(&"-G".to_owned()));
        assert!(arguments.contains(&"preview-target".to_owned()));
        assert!(arguments.contains(&"--color=always".to_owned()));
        assert!(arguments.contains(&"--icons=never".to_owned()));
        assert!(arguments.contains(&"--width=80".to_owned()));
        assert!(!arguments.iter().any(|argument| argument == "--hyperlink"));
        assert!(
            !arguments
                .iter()
                .any(|argument| argument == "--icons=always")
        );
        assert!(
            !arguments
                .iter()
                .any(|argument| argument == "--colour=always")
        );
    }

    #[test]
    fn converts_ansi_colors_to_zle_style_spans() {
        let line =
            parse_ansi_preview_line("\x1b[31;1mred\x1b[0m plain \x1b[38;2;1;2;3;4mcolor\x1b[0m");
        assert_eq!(line.text, "red plain color");
        assert_eq!(line.styles, "0:3:fg=1,bold;10:15:fg=#010203,underline");
    }
}
