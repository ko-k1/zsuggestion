use std::fs::{self, File};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{io, io::Read, io::Write};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use zsuggestion::client;
use zsuggestion::config::{Paths, Settings, completion_key_sequence};
use zsuggestion::daemon;
use zsuggestion::protocol::{Request, Response};

#[derive(Debug, Parser)]
#[command(name = "zsuggestion", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the shared per-host daemon in the foreground.
    Daemon,
    /// Record a completed command in shared history.
    Record {
        #[arg(long, required_unless_present = "stdin", conflicts_with = "stdin")]
        command: Option<String>,
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        cwd: PathBuf,
        #[arg(long)]
        exit_code: i32,
        #[arg(long, default_value = "unknown")]
        session: String,
    },
    /// Return conservative history completions for the current buffer.
    Complete {
        #[arg(long, required_unless_present = "stdin", conflicts_with = "stdin")]
        buffer: Option<String>,
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        cursor: usize,
        #[arg(long)]
        cwd: PathBuf,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
    },
    /// Fuzzy-rank shared history and installed commands with fzf.
    Fuzzy {
        #[arg(long, default_value = "")]
        query: String,
        #[arg(long)]
        cwd: PathBuf,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
    },
    /// Emit a bounded, sanitized preview for a local regular file.
    PreviewFile {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        cwd: PathBuf,
    },
    /// Execute a bounded preview for a supported read-only command.
    #[command(name = "preview-command")]
    Preview {
        #[arg(long)]
        line: String,
        #[arg(long)]
        cwd: PathBuf,
    },
    /// Import an existing Zsh history file into shared state.
    ImportHistory {
        #[arg(long)]
        file: PathBuf,
    },
    /// Print shell integration code and initialize the default config.
    Init {
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Verify config, storage, and daemon connectivity.
    Doctor,
    /// Stop the shared daemon on this host.
    Stop,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Json,
    Insert,
    Zsh,
    ZshV2,
    ZshV3,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Shell {
    Zsh,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("zsuggestion: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let paths = Paths::discover()?;

    match cli.command {
        Command::Daemon => {
            let settings = Settings::load(&paths.config_file)?;
            daemon::serve(paths, settings)
        }
        Command::Record {
            command,
            stdin,
            cwd,
            exit_code,
            session,
        } => {
            expect_success(client::request(
                &paths,
                Request::Record {
                    command: read_input(command, stdin, 64 * 1024)?,
                    cwd: cwd.to_string_lossy().into_owned(),
                    exit_code,
                    observed_at_ms: now_ms(),
                    session_id: session,
                },
            )?)?;
            Ok(())
        }
        Command::Complete {
            buffer,
            stdin,
            cursor,
            cwd,
            limit,
            format,
        } => {
            let response = client::request_idempotent(
                &paths,
                Request::Complete {
                    buffer: read_input(buffer, stdin, 64 * 1024)?,
                    cursor_byte: cursor,
                    cwd: cwd.to_string_lossy().into_owned(),
                    limit,
                },
            )?;
            write_completion(response, format)
        }
        Command::Fuzzy {
            query,
            cwd,
            limit,
            format,
        } => {
            let response = client::request_idempotent(
                &paths,
                Request::Fuzzy {
                    query,
                    cwd: cwd.to_string_lossy().into_owned(),
                    limit,
                },
            )?;
            write_completion(response, format)
        }
        Command::PreviewFile { path, cwd } => write_file_preview(path, cwd),
        Command::Preview { line, cwd } => write_command_preview(&line, cwd),
        Command::ImportHistory { file } => {
            let file = file
                .canonicalize()
                .with_context(|| format!("failed to resolve history file {}", file.display()))?;
            match client::request(
                &paths,
                Request::ImportHistory {
                    path: file.to_string_lossy().into_owned(),
                },
            )? {
                Response::Imported { imported, skipped } => {
                    if !skipped {
                        eprintln!("zsuggestion: imported {imported} history entries");
                    }
                    Ok(())
                }
                Response::Error { message } => bail!(message),
                response => bail!("unexpected daemon response: {response:?}"),
            }
        }
        Command::Init { shell: Shell::Zsh } => {
            Settings::write_default(&paths.config_file)?;
            let settings = Settings::load(&paths.config_file)?;
            print!("{}", zsh_integration(&settings)?);
            Ok(())
        }
        Command::Doctor => doctor(&paths),
        Command::Stop => match client::request_once(&paths, Request::Shutdown)? {
            Response::ShuttingDown => Ok(()),
            Response::Error { message } => bail!(message),
            response => bail!("unexpected daemon response: {response:?}"),
        },
    }
}

fn write_file_preview(path: PathBuf, cwd: PathBuf) -> Result<()> {
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
    let stdout = io::stdout();
    let mut output = stdout.lock();
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

fn write_command_preview(line: &str, cwd: PathBuf) -> Result<()> {
    let lines = command_preview_lines(line, cwd)?;
    let stdout = io::stdout();
    let mut output = stdout.lock();
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

fn command_preview_lines(line: &str, cwd: PathBuf) -> Result<Vec<CommandPreviewLine>> {
    let (executable, arguments) = command_preview_argv(line)?;

    let mut command = ProcessCommand::new(executable);
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
        arguments.push("--color=never".to_owned());
        arguments.push("--hyperlink=never".to_owned());
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

fn doctor(paths: &Paths) -> Result<()> {
    Settings::load(&paths.config_file)?;
    let response = client::request(paths, Request::Ping)?;
    let Response::Pong { version } = response else {
        bail!("unexpected daemon response: {response:?}");
    };
    println!("zsuggestion {version}");
    println!("config: {}", paths.config_file.display());
    println!("database: {}", paths.database_file.display());
    println!("socket: {}", paths.socket_file.display());
    println!("status: ready");
    Ok(())
}

fn write_completion(response: Response, format: OutputFormat) -> Result<()> {
    let Response::Completion(completion) = response else {
        return match response {
            Response::Error { message } => Err(anyhow::anyhow!(message)),
            response => Err(anyhow::anyhow!("unexpected daemon response: {response:?}")),
        };
    };
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string(&completion)?),
        OutputFormat::Insert => {
            if let Some(candidate) = completion.candidates.first() {
                print!("{}", candidate.accept_text);
            }
        }
        OutputFormat::Zsh | OutputFormat::ZshV2 | OutputFormat::ZshV3 => {
            let stdout = io::stdout();
            let mut output = stdout.lock();
            if matches!(format, OutputFormat::ZshV3) {
                output.write_all(if completion.enrichment_pending {
                    b"true"
                } else {
                    b"false"
                })?;
                output.write_all(&[0])?;
            }
            for candidate in completion.candidates {
                let source = match candidate.source {
                    zsuggestion::protocol::CandidateSource::History => "history",
                    zsuggestion::protocol::CandidateSource::Command => "command",
                    zsuggestion::protocol::CandidateSource::Filesystem => "filesystem",
                    zsuggestion::protocol::CandidateSource::Help => "help",
                };
                let kind = match candidate.kind {
                    zsuggestion::protocol::CandidateKind::History => "history",
                    zsuggestion::protocol::CandidateKind::Command => "command",
                    zsuggestion::protocol::CandidateKind::File => "file",
                    zsuggestion::protocol::CandidateKind::Directory => "directory",
                    zsuggestion::protocol::CandidateKind::Option => "option",
                    zsuggestion::protocol::CandidateKind::Subcommand => "subcommand",
                    zsuggestion::protocol::CandidateKind::Value => "value",
                };
                for field in [
                    candidate.insert_text.as_str(),
                    candidate.display.as_str(),
                    candidate.description.as_str(),
                    kind,
                    source,
                ] {
                    output.write_all(field.as_bytes())?;
                    output.write_all(&[0])?;
                }
                if matches!(format, OutputFormat::ZshV2 | OutputFormat::ZshV3) {
                    output.write_all(if candidate.description_pending {
                        b"true"
                    } else {
                        b"false"
                    })?;
                    output.write_all(&[0])?;
                }
            }
        }
    }
    Ok(())
}

fn expect_success(response: Response) -> Result<()> {
    match response {
        Response::Recorded => Ok(()),
        Response::Error { message } => bail!(message),
        response => bail!("unexpected daemon response: {response:?}"),
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn read_input(value: Option<String>, from_stdin: bool, limit: u64) -> Result<String> {
    if !from_stdin {
        return value.context("missing input value");
    }
    let mut bytes = Vec::new();
    io::stdin().take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!("stdin input exceeds {limit} bytes");
    }
    String::from_utf8(bytes).context("stdin input is not valid UTF-8")
}

fn zsh_integration(settings: &Settings) -> Result<String> {
    let completion_key = completion_key_sequence(&settings.completion.key)?;
    let completion_key_label = match settings.completion.key.as_str() {
        "ctrl-space" => "Ctrl-Space".to_owned(),
        "shift-tab" => "Shift-Tab".to_owned(),
        "tab" => "Tab".to_owned(),
        key => format!(
            "Ctrl-{}",
            key.trim_start_matches("ctrl-").to_ascii_uppercase()
        ),
    };
    let menu_width = settings.ui.menu_width.to_string();
    let max_visible = settings.ui.max_visible.to_string();
    let max_candidates = settings.completion.max_candidates.to_string();
    Ok(r#"# zsuggestion shell integration
if [[ -o interactive ]] && (( $+commands[zsuggestion] )); then
  typeset -g ZSUGGESTION_ZSH_LOADED=1
  typeset -g _ZSUGGESTION_COMMAND=""
  typeset -g _ZSUGGESTION_COMMAND_CWD=""
  typeset -g _ZSUGGESTION_MENU_ACTIVE=0
  typeset -g _ZSUGGESTION_MENU_INDEX=1
  typeset -g _ZSUGGESTION_MENU_START=1
  typeset -g _ZSUGGESTION_MENU_BUFFER=""
  typeset -g _ZSUGGESTION_MENU_OWNS_DISPLAY=0
  typeset -g _ZSUGGESTION_MENU_REQUEST_BUFFER=""
  typeset -g _ZSUGGESTION_MENU_REQUEST_CWD=""
  typeset -g _ZSUGGESTION_MENU_REQUEST_CURSOR=0
  typeset -g _ZSUGGESTION_MENU_REQUEST_FD=-1
  typeset -g _ZSUGGESTION_MENU_INFLIGHT_BUFFER=""
  typeset -g _ZSUGGESTION_MENU_INFLIGHT_CURSOR=0
  typeset -g _ZSUGGESTION_MENU_REQUEST_DIRTY=0
  typeset -g _ZSUGGESTION_MENU_REFRESH_TICKS=0
  typeset -g _ZSUGGESTION_MENU_GENERATION=0
  typeset -g _ZSUGGESTION_MENU_INFLIGHT_GENERATION=0
  typeset -g _ZSUGGESTION_MENU_KEYTIMEOUT=-1
  typeset -g _ZSUGGESTION_MENU_RESTORE_INDEX=1
  typeset -g _ZSUGGESTION_RESTORE_HIGHLIGHTS=0
  typeset -g _ZSUGGESTION_CAPTURE_FOREIGN_HIGHLIGHTS=0
  typeset -g _ZSUGGESTION_MENU_TICK_FD=-1
  typeset -g _ZSUGGESTION_HAS_ZSELECT=0
  typeset -g _ZSUGGESTION_NATIVE_REQUEST_FD=-1
  typeset -g _ZSUGGESTION_NATIVE_REQUEST_PID=-1
  typeset -g _ZSUGGESTION_NATIVE_REQUEST_TICKS=0
  typeset -g _ZSUGGESTION_NATIVE_START_TICKS=0
  typeset -g _ZSUGGESTION_NATIVE_INFLIGHT_GENERATION=0
  typeset -g _ZSUGGESTION_NATIVE_REQUESTED=0
  typeset -g _ZSUGGESTION_NATIVE_INFLIGHT_BUFFER=""
  typeset -g _ZSUGGESTION_NATIVE_INFLIGHT_CURSOR=0
  typeset -g _ZSUGGESTION_IN_NATIVE_COMPLETION=0
  typeset -g _ZSUGGESTION_IN_BRACKETED_PASTE=0
  typeset -g _ZSUGGESTION_FUZZY_ACTIVE=0
  typeset -g _ZSUGGESTION_FUZZY_BASE=""
  typeset -g _ZSUGGESTION_FUZZY_QUERY=""
  typeset -g _ZSUGGESTION_FUZZY_KEYTIMEOUT=-1
  typeset -g _ZSUGGESTION_FUZZY_PREVIOUS_KEYMAP=""
  typeset -g _ZSUGGESTION_PREVIEW_FD=-1
  typeset -g _ZSUGGESTION_PREVIEW_PID=-1
  typeset -g _ZSUGGESTION_PREVIEW_TICKS=0
  typeset -g _ZSUGGESTION_PREVIEW_TARGET=""
  typeset -g _ZSUGGESTION_PREVIEW_PATH=""
  typeset -g _ZSUGGESTION_PREVIEW_COMMAND=""
  typeset -ga _ZSUGGESTION_PREVIEW_LINES=()
  typeset -ga _ZSUGGESTION_PREVIEW_STYLES=()
  typeset -g _ZSUGGESTION_UTF8_UI=0
  typeset -g _ZSUGGESTION_CHARMAP="${(U)$(command locale charmap 2>/dev/null)}"
  [[ "$_ZSUGGESTION_CHARMAP" == *UTF-8* || "$_ZSUGGESTION_CHARMAP" == *UTF8* ]] && _ZSUGGESTION_UTF8_UI=1
  unset _ZSUGGESTION_CHARMAP
  typeset -ga _ZSUGGESTION_MENU_ACCEPTS=()
  typeset -ga _ZSUGGESTION_MENU_DISPLAYS=()
  typeset -ga _ZSUGGESTION_MENU_DESCRIPTIONS=()
  typeset -ga _ZSUGGESTION_MENU_KINDS=()
  typeset -ga _ZSUGGESTION_MENU_SOURCES=()
  typeset -ga _ZSUGGESTION_FOREIGN_HIGHLIGHTS=()
  typeset -ga _ZSUGGESTION_RENDERED_HIGHLIGHTS=()
  typeset -ga _ZSUGGESTION_DAEMON_ACCEPTS=()
  typeset -ga _ZSUGGESTION_DAEMON_DISPLAYS=()
  typeset -ga _ZSUGGESTION_DAEMON_DESCRIPTIONS=()
  typeset -ga _ZSUGGESTION_DAEMON_KINDS=()
  typeset -ga _ZSUGGESTION_DAEMON_SOURCES=()
  typeset -ga _ZSUGGESTION_NATIVE_ACCEPTS=()
  typeset -ga _ZSUGGESTION_NATIVE_DISPLAYS=()
  typeset -ga _ZSUGGESTION_NATIVE_DESCRIPTIONS=()

  _zsuggestion_preexec() {
    _ZSUGGESTION_COMMAND="$1"
    _ZSUGGESTION_COMMAND_CWD="$PWD"
    return 0
  }

  _zsuggestion_precmd() {
    local exit_code=$?
    if [[ -n "$_ZSUGGESTION_COMMAND" ]]; then
      local session_id="${HOST:-unknown}:${TMUX_PANE:-${SSH_TTY:-local}}:$$"
      print -rn -- "$_ZSUGGESTION_COMMAND" | command zsuggestion record \
        --stdin \
        --cwd "$_ZSUGGESTION_COMMAND_CWD" \
        --exit-code "$exit_code" \
        --session "$session_id" >/dev/null 2>&1 &!
      _ZSUGGESTION_COMMAND=""
      _ZSUGGESTION_COMMAND_CWD=""
    fi
    return 0
  }

  _zsuggestion_menu_cancel_query() {
    local fd="$_ZSUGGESTION_MENU_REQUEST_FD"
    (( _ZSUGGESTION_MENU_GENERATION++ ))
    if (( fd >= 0 )); then
      zle -F "$fd" 2>/dev/null
      exec {fd}<&-
    fi
    _ZSUGGESTION_MENU_REQUEST_FD=-1
    _ZSUGGESTION_MENU_INFLIGHT_BUFFER=""
    _ZSUGGESTION_MENU_INFLIGHT_CURSOR=0
  }

  _zsuggestion_native_cancel() {
    local fd="$_ZSUGGESTION_NATIVE_REQUEST_FD"
    (( _ZSUGGESTION_MENU_GENERATION++ ))
    if (( fd >= 0 )); then
      zle -F "$fd" 2>/dev/null
      exec {fd}<&-
    fi
    if (( _ZSUGGESTION_NATIVE_REQUEST_PID > 0 )); then
      kill "$_ZSUGGESTION_NATIVE_REQUEST_PID" 2>/dev/null
    fi
    _ZSUGGESTION_NATIVE_REQUEST_FD=-1
    _ZSUGGESTION_NATIVE_REQUEST_PID=-1
    _ZSUGGESTION_NATIVE_REQUEST_TICKS=0
    _ZSUGGESTION_NATIVE_START_TICKS=0
    _ZSUGGESTION_NATIVE_INFLIGHT_BUFFER=""
    _ZSUGGESTION_NATIVE_INFLIGHT_CURSOR=0
  }

  _zsuggestion_preview_cancel() {
    local fd="$_ZSUGGESTION_PREVIEW_FD"
    if (( fd >= 0 )); then
      zle -F "$fd" 2>/dev/null
      exec {fd}<&-
    fi
    (( _ZSUGGESTION_PREVIEW_PID > 0 )) && kill "$_ZSUGGESTION_PREVIEW_PID" 2>/dev/null
    _ZSUGGESTION_PREVIEW_FD=-1
    _ZSUGGESTION_PREVIEW_PID=-1
    _ZSUGGESTION_PREVIEW_TICKS=0
  }

  _zsuggestion_preview_clear() {
    _zsuggestion_preview_cancel
    _ZSUGGESTION_PREVIEW_TARGET=""
    _ZSUGGESTION_PREVIEW_PATH=""
    _ZSUGGESTION_PREVIEW_COMMAND=""
    _ZSUGGESTION_PREVIEW_LINES=()
    _ZSUGGESTION_PREVIEW_STYLES=()
  }

  _zsuggestion_preview_consider() {
    local display source target path command_name alias_command
    local -a display_words
    if (( ${COLUMNS:-80} < 100 || ! _ZSUGGESTION_MENU_ACTIVE )); then
      _zsuggestion_preview_clear
      return
    fi
    display="${_ZSUGGESTION_MENU_DISPLAYS[$_ZSUGGESTION_MENU_INDEX]}"
    source="${_ZSUGGESTION_MENU_SOURCES[$_ZSUGGESTION_MENU_INDEX]}"
    target="${source}:${display}"
    [[ "$target" == "$_ZSUGGESTION_PREVIEW_TARGET" ]] && return
    _zsuggestion_preview_clear
    _ZSUGGESTION_PREVIEW_TARGET="$target"
    command_name="${${display%%[[:space:]]*}:t}"
    alias_command="${aliases[ls]:-}"
    if [[ "$command_name" == ls &&
          ( "$alias_command" == eza || "$alias_command" == "eza "* ) ]]; then
      _ZSUGGESTION_PREVIEW_COMMAND="${alias_command}${display#$command_name}"
    elif [[ "$command_name" == ls || "$command_name" == gls || "$command_name" == eza ]]; then
      _ZSUGGESTION_PREVIEW_COMMAND="$display"
    elif [[ "$source" == native || "$source" == filesystem ]]; then
      display_words=("${(z)display}")
      path="${(Q)display_words[-1]}"
      [[ -n "$path" ]] || return
      [[ "$path" == "~/"* ]] && path="$HOME/${path#\~/}"
      _ZSUGGESTION_PREVIEW_PATH="$path"
    fi
  }

  _zsuggestion_preview_ready() {
    local fd="$1" line style response_target
    local styled=$(( ${#_ZSUGGESTION_PREVIEW_COMMAND} > 0 ))
    local -a lines styles
    if (( fd != _ZSUGGESTION_PREVIEW_FD )); then
      zle -F "$fd" 2>/dev/null
      exec {fd}<&-
      return 0
    fi
    zle -F "$fd"
    if ! IFS= read -r -u "$fd" -d '' response_target ||
       [[ "$response_target" != "$_ZSUGGESTION_PREVIEW_TARGET" ]]; then
      exec {fd}<&-
      _ZSUGGESTION_PREVIEW_FD=-1
      _ZSUGGESTION_PREVIEW_PID=-1
      _ZSUGGESTION_PREVIEW_TICKS=0
      return 0
    fi
    if (( styled )); then
      while IFS= read -r -u "$fd" -d '' line &&
            IFS= read -r -u "$fd" -d '' style; do
        lines+=("$line")
        styles+=("$style")
        (( ${#lines} >= 8 )) && break
      done
    else
      while IFS= read -r -u "$fd" -d '' line; do
        lines+=("$line")
        styles+=("")
        (( ${#lines} >= 8 )) && break
      done
    fi
    exec {fd}<&-
    _ZSUGGESTION_PREVIEW_FD=-1
    _ZSUGGESTION_PREVIEW_PID=-1
    _ZSUGGESTION_PREVIEW_TICKS=0
    _ZSUGGESTION_PREVIEW_PATH=""
    _ZSUGGESTION_PREVIEW_COMMAND=""
    if (( ${#lines} )); then
      _ZSUGGESTION_PREVIEW_LINES=("${lines[@]}")
      _ZSUGGESTION_PREVIEW_STYLES=("${styles[@]}")
    else
      _ZSUGGESTION_PREVIEW_LINES=("Preview unavailable")
      _ZSUGGESTION_PREVIEW_STYLES=("")
    fi
    _zsuggestion_menu_publish
  }

  _zsuggestion_menu_cancel_request() {
    _zsuggestion_menu_cancel_query
    _zsuggestion_native_cancel
    _ZSUGGESTION_MENU_REQUEST_BUFFER=""
    _ZSUGGESTION_MENU_REQUEST_CWD=""
    _ZSUGGESTION_MENU_REQUEST_CURSOR=0
    _ZSUGGESTION_MENU_REQUEST_DIRTY=0
    _ZSUGGESTION_MENU_REFRESH_TICKS=0
    _ZSUGGESTION_MENU_RESTORE_INDEX=1
    _ZSUGGESTION_NATIVE_REQUESTED=0
    _ZSUGGESTION_NATIVE_START_TICKS=0
  }

  _zsuggestion_fuzzy_reset() {
    if (( _ZSUGGESTION_FUZZY_ACTIVE && _ZSUGGESTION_FUZZY_KEYTIMEOUT >= 0 )); then
      KEYTIMEOUT=$_ZSUGGESTION_FUZZY_KEYTIMEOUT
    fi
    if (( _ZSUGGESTION_FUZZY_ACTIVE )) && [[ -n "$_ZSUGGESTION_FUZZY_PREVIOUS_KEYMAP" ]]; then
      zle -K "$_ZSUGGESTION_FUZZY_PREVIOUS_KEYMAP" 2>/dev/null
    fi
    _ZSUGGESTION_FUZZY_ACTIVE=0
    _ZSUGGESTION_FUZZY_BASE=""
    _ZSUGGESTION_FUZZY_QUERY=""
    _ZSUGGESTION_FUZZY_KEYTIMEOUT=-1
    _ZSUGGESTION_FUZZY_PREVIOUS_KEYMAP=""
  }

  _zsuggestion_menu_clear() {
    local preserve_buffer="${1:-0}"
    if (( _ZSUGGESTION_MENU_KEYTIMEOUT >= 0 )); then
      KEYTIMEOUT=$_ZSUGGESTION_MENU_KEYTIMEOUT
      _ZSUGGESTION_MENU_KEYTIMEOUT=-1
    fi
    _zsuggestion_menu_cancel_request
    _ZSUGGESTION_MENU_ACTIVE=0
    _ZSUGGESTION_MENU_INDEX=1
    _ZSUGGESTION_MENU_START=1
    _ZSUGGESTION_RESTORE_HIGHLIGHTS=0
    _ZSUGGESTION_CAPTURE_FOREIGN_HIGHLIGHTS=0
    _zsuggestion_fuzzy_reset
    _zsuggestion_preview_clear
    (( preserve_buffer )) || _ZSUGGESTION_MENU_BUFFER=""
    _ZSUGGESTION_MENU_ACCEPTS=()
    _ZSUGGESTION_MENU_DISPLAYS=()
    _ZSUGGESTION_MENU_DESCRIPTIONS=()
    _ZSUGGESTION_MENU_KINDS=()
    _ZSUGGESTION_MENU_SOURCES=()
    _ZSUGGESTION_DAEMON_ACCEPTS=()
    _ZSUGGESTION_DAEMON_DISPLAYS=()
    _ZSUGGESTION_DAEMON_DESCRIPTIONS=()
    _ZSUGGESTION_DAEMON_KINDS=()
    _ZSUGGESTION_DAEMON_SOURCES=()
    _ZSUGGESTION_NATIVE_ACCEPTS=()
    _ZSUGGESTION_NATIVE_DISPLAYS=()
    _ZSUGGESTION_NATIVE_DESCRIPTIONS=()
    if (( _ZSUGGESTION_MENU_OWNS_DISPLAY )); then
      POSTDISPLAY=""
      region_highlight=( "${(@)region_highlight:#*memo=zsuggestion*}" )
      _ZSUGGESTION_MENU_OWNS_DISPLAY=0
    fi
  }

  _zsuggestion_fuzzy_start() {
    (( CURSOR == ${#BUFFER} )) || return 1
    _ZSUGGESTION_FUZZY_KEYTIMEOUT=$KEYTIMEOUT
    _ZSUGGESTION_FUZZY_PREVIOUS_KEYMAP="$KEYMAP"
    KEYTIMEOUT=1
    _ZSUGGESTION_FUZZY_ACTIVE=1
    zle -K zsuggestion-fuzzy
    _ZSUGGESTION_FUZZY_BASE="$BUFFER"
    _ZSUGGESTION_FUZZY_QUERY=""
    _ZSUGGESTION_MENU_BUFFER="$BUFFER"
    _zsuggestion_menu_schedule
    _zsuggestion_menu_render
  }

  _zsuggestion_fuzzy_refresh() {
    BUFFER="${_ZSUGGESTION_FUZZY_BASE}${_ZSUGGESTION_FUZZY_QUERY}"
    CURSOR=${#BUFFER}
    _ZSUGGESTION_MENU_BUFFER="$BUFFER"
    _zsuggestion_menu_schedule
    _zsuggestion_menu_render
  }

  _zsuggestion_menu_render() {
    local index display description kind kind_label icon marker row fill ghost=""
    local separator="·" ellipsis="…"
    local selected_marker="▌ " history_icon="↺" command_icon="❯" native_icon="⇥"
    local fuzzy_ghost_prefix="  → "
    local file_icon="·" directory_icon="▸" option_icon="-" subcommand_icon="›" value_icon="="
    if (( ! _ZSUGGESTION_UTF8_UI )); then
      separator="|"
      ellipsis="~"
      selected_marker="| "
      history_icon="H"
      command_icon="$"
      native_icon="T"
      file_icon="F"
      directory_icon="D"
      option_icon="O"
      subcommand_icon="S"
      value_icon="V"
      fuzzy_ghost_prefix="  -> "
    fi
    local input="${BUFFER:-$_ZSUGGESTION_MENU_REQUEST_BUFFER}"
    local cursor_position=$CURSOR
    if [[ -z "$BUFFER" && -n "$_ZSUGGESTION_MENU_REQUEST_BUFFER" ]]; then
      cursor_position=$_ZSUGGESTION_MENU_REQUEST_CURSOR
    fi
    local total=${#_ZSUGGESTION_MENU_DISPLAYS}
    local selected="$_ZSUGGESTION_MENU_DISPLAYS[$_ZSUGGESTION_MENU_INDEX]"
    if (( cursor_position == ${#input} )); then
      if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
        [[ -n "$selected" ]] && ghost="${fuzzy_ghost_prefix}${selected}"
      elif [[ -n "$input" && "$selected" == "$input"* ]]; then
        ghost="${selected[${#input}+1,-1]}"
      fi
    fi
    region_highlight=( "${(@)region_highlight:#*memo=zsuggestion*}" )
    POSTDISPLAY="$ghost"
    local buffer_end=${#input}
    if (( _ZSUGGESTION_FUZZY_ACTIVE && ${#_ZSUGGESTION_FUZZY_QUERY} )); then
      local query_start=${#_ZSUGGESTION_FUZZY_BASE}
      region_highlight+=("$query_start $buffer_end fg=__ZSUGGESTION_UI_ACCENT__,bold memo=zsuggestion")
    fi
    if [[ -n "$ghost" ]]; then
      region_highlight+=("$buffer_end $(( buffer_end + ${#ghost} )) fg=__ZSUGGESTION_UI_GHOST__ memo=zsuggestion")
    fi
    local box_width=$(( ${COLUMNS:-80} - 2 ))
    (( box_width > __ZSUGGESTION_UI_MENU_WIDTH__ )) && box_width=__ZSUGGESTION_UI_MENU_WIDTH__
    if (( box_width < 40 )); then
      _zsuggestion_preview_clear
      local compact_width=$(( ${COLUMNS:-80} - 1 ))
      if (( total > 0 && compact_width >= 8 )); then
        local compact_indent=$(( __ZSUGGESTION_UI_PROMPT_OFFSET__ + cursor_position ))
        local compact_max_indent=$(( ${COLUMNS:-80} - compact_width - 1 ))
        (( compact_indent > compact_max_indent )) && compact_indent=$compact_max_indent
        (( compact_indent < 0 )) && compact_indent=0
        local compact_padding compact_display="$selected" compact_completion=""
        printf -v compact_padding '%*s' "$compact_indent" ""
        if (( ${#compact_display} > compact_width - 4 )); then
          if (( ! _ZSUGGESTION_FUZZY_ACTIVE )) && [[ "$compact_display" == "$input"* ]]; then
            compact_completion="${compact_display[${#input}+1,-1]}"
          fi
          if [[ -n "$compact_completion" ]] &&
             (( ${#compact_completion} <= compact_width - 6 )); then
            compact_display="${ellipsis} ${compact_completion}"
          else
            local compact_tail_width=$(( compact_width - 4 - ${#ellipsis} ))
            local compact_tail_start=$(( ${#compact_display} - compact_tail_width + 1 ))
            compact_display="${ellipsis}${compact_display[$compact_tail_start,-1]}"
          fi
        fi
        kind="$_ZSUGGESTION_MENU_KINDS[$_ZSUGGESTION_MENU_INDEX]"
        case "$kind" in
          history) icon="$history_icon" ;;
          command) icon="$command_icon" ;;
          native) icon="$native_icon" ;;
          file) icon="$file_icon" ;;
          directory) icon="$directory_icon" ;;
          option) icon="$option_icon" ;;
          subcommand) icon="$subcommand_icon" ;;
          value) icon="$value_icon" ;;
          *) icon="." ;;
        esac
        row="${selected_marker}${icon} ${compact_display}"
        local compact_start=$(( ${#input} + ${#POSTDISPLAY} + 1 + compact_indent ))
        POSTDISPLAY+=$'\n'"${compact_padding}${row}"
        region_highlight+=("$compact_start $(( compact_start + ${#row} )) bg=__ZSUGGESTION_UI_SELECTED_BACKGROUND__,fg=__ZSUGGESTION_UI_SELECTED_TEXT__ memo=zsuggestion")
        region_highlight+=("$compact_start $(( compact_start + 2 )) bg=__ZSUGGESTION_UI_SELECTED_BACKGROUND__,fg=__ZSUGGESTION_UI_ACCENT__,bold memo=zsuggestion")
      fi
      _ZSUGGESTION_MENU_OWNS_DISPLAY=1
      return 0
    fi

    if (( total == 0 )); then
      region_highlight=( "${(@)region_highlight:#*memo=zsuggestion*}" )
      POSTDISPLAY=""
      if (( _ZSUGGESTION_FUZZY_ACTIVE && ${#_ZSUGGESTION_FUZZY_QUERY} )); then
        local query_start=${#_ZSUGGESTION_FUZZY_BASE}
        region_highlight+=("$query_start ${#input} fg=__ZSUGGESTION_UI_ACCENT__,bold memo=zsuggestion")
      fi
      _ZSUGGESTION_MENU_OWNS_DISPLAY=1
      return 0
    fi
    local window_size=$total
    (( window_size > __ZSUGGESTION_UI_MAX_VISIBLE__ )) && window_size=__ZSUGGESTION_UI_MAX_VISIBLE__
    local max_start=$(( total - window_size + 1 ))
    if (( window_size > 2 )); then
      (( _ZSUGGESTION_MENU_INDEX <= _ZSUGGESTION_MENU_START && _ZSUGGESTION_MENU_START > 1 )) && \
        _ZSUGGESTION_MENU_START=$(( _ZSUGGESTION_MENU_INDEX - 1 ))
      (( _ZSUGGESTION_MENU_INDEX >= _ZSUGGESTION_MENU_START + window_size - 1 )) && \
        _ZSUGGESTION_MENU_START=$(( _ZSUGGESTION_MENU_INDEX - window_size + 2 ))
    else
      (( _ZSUGGESTION_MENU_INDEX < _ZSUGGESTION_MENU_START )) && _ZSUGGESTION_MENU_START=$_ZSUGGESTION_MENU_INDEX
      (( _ZSUGGESTION_MENU_INDEX >= _ZSUGGESTION_MENU_START + window_size )) && \
        _ZSUGGESTION_MENU_START=$(( _ZSUGGESTION_MENU_INDEX - window_size + 1 ))
    fi
    (( _ZSUGGESTION_MENU_START < 1 )) && _ZSUGGESTION_MENU_START=1
    (( _ZSUGGESTION_MENU_START > max_start )) && _ZSUGGESTION_MENU_START=$max_start
    local end=$(( _ZSUGGESTION_MENU_START + window_size - 1 ))

    local description_width=24
    (( box_width < 64 )) && description_width=16
    local kind_width=6
    local title_width=$(( box_width - description_width - kind_width - 7 ))
    local thumb="█" track="│"
    if (( ! _ZSUGGESTION_UTF8_UI )); then
      thumb='#'
      track='|'
    fi
    local thumb_size=0 thumb_pos=0 scrollbar_char="$track"
    if (( total > window_size )); then
      thumb_size=$(( window_size * window_size / total ))
      (( thumb_size < 1 )) && thumb_size=1
      (( thumb_size > window_size )) && thumb_size=$window_size
      thumb_pos=$(( (_ZSUGGESTION_MENU_START - 1) * (window_size - thumb_size) / (total - window_size) ))
    fi

    local indent=$(( __ZSUGGESTION_UI_PROMPT_OFFSET__ + cursor_position ))
    local max_indent=$(( ${COLUMNS:-80} - box_width - 1 ))
    (( indent > max_indent )) && indent=$max_indent
    (( indent < 0 )) && indent=0
    local padding
    printf -v padding '%*s' "$indent" ""

    _zsuggestion_preview_consider

    for (( index = _ZSUGGESTION_MENU_START; index <= end; index++ )); do
      display="$_ZSUGGESTION_MENU_DISPLAYS[$index]"
      local display_truncated=0 completion_tail=""
      if (( ${#display} > title_width )); then
        display_truncated=1
        if (( ! _ZSUGGESTION_FUZZY_ACTIVE )) && [[ "$display" == "$input"* ]]; then
          completion_tail="${display[${#input}+1,-1]}"
        fi
        if [[ -n "$completion_tail" ]] && (( ${#completion_tail} <= title_width - 2 )); then
          display="${ellipsis} ${completion_tail}"
        else
          local tail_width=$(( title_width - ${#ellipsis} ))
          local tail_start=$(( ${#display} - tail_width + 1 ))
          display="${ellipsis}${display[$tail_start,-1]}"
        fi
      fi
      description="$_ZSUGGESTION_MENU_DESCRIPTIONS[$index]"
      if (( ${#description} > description_width )); then
        description="${description[1,$(( description_width - 1 ))]}${ellipsis}"
      fi
      kind="$_ZSUGGESTION_MENU_KINDS[$index]"
      case "$kind" in
        history) icon="$history_icon" ;;
        command) icon="$command_icon" ;;
        native) icon="$native_icon" ;;
        file) icon="$file_icon" ;;
        directory) icon="$directory_icon" ;;
        option) icon="$option_icon" ;;
        subcommand) icon="$subcommand_icon" ;;
        value) icon="$value_icon" ;;
        *) icon="." ;;
      esac
      case "$kind" in
        history) kind_label="Hist" ;;
        command) kind_label="Cmd" ;;
        native) kind_label="Native" ;;
        file) kind_label="File" ;;
        directory) kind_label="Dir" ;;
        option) kind_label="Opt" ;;
        subcommand) kind_label="Sub" ;;
        value) kind_label="Value" ;;
        *) kind_label="" ;;
      esac
      marker="  "
      (( index == _ZSUGGESTION_MENU_INDEX )) && marker="$selected_marker"
      scrollbar_char="$track"
      if (( thumb_size > 0 )); then
        local row_offset=$(( index - _ZSUGGESTION_MENU_START ))
        (( row_offset >= thumb_pos && row_offset < thumb_pos + thumb_size )) && scrollbar_char="$thumb"
      fi
      printf -v row '%s%s %-*s %-*s %*s%s' \
        "$marker" "$icon" "$title_width" "$display" \
        "$description_width" "$description" "$kind_width" "$kind_label" "$scrollbar_char"

      line_start=$(( ${#input} + ${#POSTDISPLAY} + 1 + indent ))
      POSTDISPLAY+=$'\n'"${padding}${row}"
      if (( index == _ZSUGGESTION_MENU_INDEX )); then
        region_highlight+=("$(( line_start )) $(( line_start + ${#row} - 1 )) bg=__ZSUGGESTION_UI_SELECTED_BACKGROUND__,fg=__ZSUGGESTION_UI_SELECTED_TEXT__ memo=zsuggestion")
        region_highlight+=("$(( line_start )) $(( line_start + 2 )) bg=__ZSUGGESTION_UI_SELECTED_BACKGROUND__,fg=__ZSUGGESTION_UI_ACCENT__,bold memo=zsuggestion")
        region_highlight+=("$(( line_start + 2 )) $(( line_start + 4 )) bg=__ZSUGGESTION_UI_SELECTED_BACKGROUND__,fg=__ZSUGGESTION_UI_ACCENT__ memo=zsuggestion")
      else
        region_highlight+=("$(( line_start )) $(( line_start + ${#row} - 1 )) fg=__ZSUGGESTION_UI_TEXT__ memo=zsuggestion")
        region_highlight+=("$(( line_start )) $(( line_start + 4 )) fg=__ZSUGGESTION_UI_MUTED__ memo=zsuggestion")
      fi
      local rail_attrs="fg=__ZSUGGESTION_UI_MUTED__"
      [[ "$scrollbar_char" == "$thumb" ]] && rail_attrs="fg=__ZSUGGESTION_UI_SELECTED_TEXT__"
      (( index == _ZSUGGESTION_MENU_INDEX )) && rail_attrs="${rail_attrs},bg=__ZSUGGESTION_UI_SELECTED_BACKGROUND__"
      region_highlight+=("$(( line_start + ${#row} - 1 )) $(( line_start + ${#row} )) $rail_attrs memo=zsuggestion")

      local match_length=0
      (( ! _ZSUGGESTION_FUZZY_ACTIVE && ! display_truncated )) && match_length=${#input}
      (( match_length > title_width )) && match_length=$title_width
      if (( match_length > 0 )); then
        if (( index == _ZSUGGESTION_MENU_INDEX )); then
          region_highlight+=("$(( line_start + 4 )) $(( line_start + 4 + match_length )) bg=__ZSUGGESTION_UI_SELECTED_BACKGROUND__,fg=__ZSUGGESTION_UI_SELECTED_TEXT__,bold memo=zsuggestion")
        else
          region_highlight+=("$(( line_start + 4 )) $(( line_start + 4 + match_length )) fg=__ZSUGGESTION_UI_SELECTED_TEXT__,bold memo=zsuggestion")
        fi
      fi
      local description_start=$(( line_start + 5 + title_width ))
      if (( index == _ZSUGGESTION_MENU_INDEX )); then
        region_highlight+=("$description_start $(( description_start + description_width )) bg=__ZSUGGESTION_UI_SELECTED_BACKGROUND__,fg=__ZSUGGESTION_UI_SELECTED_SOURCE__ memo=zsuggestion")
      else
        region_highlight+=("$description_start $(( description_start + description_width )) fg=__ZSUGGESTION_UI_MUTED__ memo=zsuggestion")
      fi
      local kind_start=$(( description_start + description_width + 1 ))
      if (( index == _ZSUGGESTION_MENU_INDEX )); then
        region_highlight+=("$kind_start $(( kind_start + kind_width )) bg=__ZSUGGESTION_UI_SELECTED_BACKGROUND__,fg=__ZSUGGESTION_UI_ACCENT__,bold memo=zsuggestion")
      else
        region_highlight+=("$kind_start $(( kind_start + kind_width )) fg=__ZSUGGESTION_UI_MUTED__ memo=zsuggestion")
      fi
    done

    if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      local footer=" Esc exit ${separator} C-K up ${separator} C-N down ${separator} Tab choose ${separator} Enter run "
      line_start=$(( ${#input} + ${#POSTDISPLAY} + 1 + indent ))
      POSTDISPLAY+=$'\n'"${padding}${footer}"
      region_highlight+=("$line_start $(( line_start + ${#footer} )) fg=__ZSUGGESTION_UI_MUTED__ memo=zsuggestion")
    else
      local footer=" Up/Down select ${separator} Tab part ${separator} __ZSUGGESTION_COMPLETION_KEY_LABEL__ full "
      line_start=$(( ${#input} + ${#POSTDISPLAY} + 1 + indent ))
      POSTDISPLAY+=$'\n'"${padding}${footer}"
      region_highlight+=("$line_start $(( line_start + ${#footer} )) fg=__ZSUGGESTION_UI_MUTED__ memo=zsuggestion")
    fi
    local preview_source="${_ZSUGGESTION_MENU_SOURCES[$_ZSUGGESTION_MENU_INDEX]}"
    local preview_description="${_ZSUGGESTION_MENU_DESCRIPTIONS[$_ZSUGGESTION_MENU_INDEX]}"
    local show_preview=0
    if [[ ${#_ZSUGGESTION_PREVIEW_LINES} -gt 0 &&
          "${_ZSUGGESTION_PREVIEW_LINES[1]}" != "Preview unavailable" &&
          "${_ZSUGGESTION_PREVIEW_LINES[1]}" != "Preview timed out" ]]; then
      show_preview=1
    elif [[ "$preview_source" == command && -n "$preview_description" &&
          "$preview_description" != "System command" &&
          "$preview_description" != "Executable installed by "* ]]; then
      show_preview=1
    fi
    if (( ${COLUMNS:-80} >= 100 && show_preview )); then
      local preview_width=$(( ${COLUMNS:-80} - indent - 1 ))
      (( preview_width > 96 )) && preview_width=96
      if (( preview_width >= 40 )); then
        local preview_label="Preview: ${selected}"
        local preview_style style_span style_start style_end style_value style_rest
        local preview_content_width=$(( preview_width - 2 ))
        local -a preview_lines
        if (( ${#_ZSUGGESTION_PREVIEW_LINES} )); then
          preview_lines=("${_ZSUGGESTION_PREVIEW_LINES[@]}")
        else
          preview_lines=("Source: ${_ZSUGGESTION_MENU_SOURCES[$_ZSUGGESTION_MENU_INDEX]} / ${_ZSUGGESTION_MENU_KINDS[$_ZSUGGESTION_MENU_INDEX]}")
          preview_lines+=("$preview_description")
        fi
        (( ${#preview_label} > preview_width )) && \
          preview_label="${preview_label[1,$(( preview_width - 1 ))]}${ellipsis}"
        local line_start
        line_start=$(( ${#input} + ${#POSTDISPLAY} + 1 + indent ))
        POSTDISPLAY+=$'\n'"${padding}${preview_label}"
        region_highlight+=("$line_start $(( line_start + ${#preview_label} )) fg=__ZSUGGESTION_UI_MUTED__ memo=zsuggestion")
        local preview_index
        for (( preview_index = 1; preview_index <= ${#preview_lines} && preview_index <= 8; preview_index++ )); do
          preview_line="${preview_lines[$preview_index]}"
          (( ${#preview_line} > preview_content_width )) && \
            preview_line="${preview_line[1,$(( preview_content_width - 1 ))]}${ellipsis}"
          line_start=$(( ${#input} + ${#POSTDISPLAY} + 1 + indent ))
          POSTDISPLAY+=$'\n'"${padding}${preview_line}"
          region_highlight+=("$line_start $(( line_start + ${#preview_line} )) fg=__ZSUGGESTION_UI_TEXT__ memo=zsuggestion")
          preview_style="${_ZSUGGESTION_PREVIEW_STYLES[$preview_index]}"
          for style_span in ${(s:;:)preview_style}; do
            style_start="${style_span%%:*}"
            style_rest="${style_span#*:}"
            style_end="${style_rest%%:*}"
            style_value="${style_rest#*:}"
            [[ "$style_start" == <-> && "$style_end" == <-> && -n "$style_value" ]] || continue
            (( style_start >= ${#preview_line} )) && continue
            (( style_end > ${#preview_line} )) && style_end=${#preview_line}
            (( style_start < style_end )) && \
              region_highlight+=("$(( line_start + style_start )) $(( line_start + style_end )) ${style_value} memo=zsuggestion")
          done
        done
      fi
    fi
    _ZSUGGESTION_MENU_OWNS_DISPLAY=1
  }

  _zsuggestion_menu_query() {
    local LC_ALL=C cursor_byte accept display description kind source
    _zsuggestion_menu_cancel_request
    cursor_byte=${#LBUFFER}
    _ZSUGGESTION_MENU_BUFFER="$BUFFER"
    _ZSUGGESTION_MENU_ACCEPTS=()
    _ZSUGGESTION_MENU_DISPLAYS=()
    _ZSUGGESTION_MENU_DESCRIPTIONS=()
    _ZSUGGESTION_MENU_KINDS=()
    _ZSUGGESTION_MENU_SOURCES=()
    while IFS= read -r -d '' accept &&
          IFS= read -r -d '' display &&
          IFS= read -r -d '' description &&
          IFS= read -r -d '' kind &&
          IFS= read -r -d '' source; do
      _ZSUGGESTION_MENU_ACCEPTS+=("$accept")
      _ZSUGGESTION_MENU_DISPLAYS+=("$display")
      _ZSUGGESTION_MENU_DESCRIPTIONS+=("$description")
      _ZSUGGESTION_MENU_KINDS+=("$kind")
      _ZSUGGESTION_MENU_SOURCES+=("$source")
    done < <(print -rn -- "$BUFFER" | command zsuggestion complete \
      --stdin \
      --cursor "$cursor_byte" \
      --cwd "$PWD" \
      --format zsh 2>/dev/null)
    if (( ${#_ZSUGGESTION_MENU_ACCEPTS} == 0 )); then
      _zsuggestion_menu_clear 1
      return 1
    fi
    _ZSUGGESTION_MENU_ACTIVE=1
    _ZSUGGESTION_MENU_INDEX=1
    _ZSUGGESTION_MENU_START=1
    _zsuggestion_menu_render
    return 0
  }

  _zsuggestion_next_segment() {
    local value="$1" character
    local index boundary next quote=""
    local saw_non_whitespace=0 escaped=0 bracket_depth=0
    REPLY="$value"
    for (( index = 1; index <= ${#value}; index++ )); do
      character="${value[$index]}"
      if (( escaped )); then
        escaped=0
        saw_non_whitespace=1
        continue
      fi
      if [[ "$character" == \\ && "$quote" != "'" ]]; then
        escaped=1
        saw_non_whitespace=1
        continue
      fi
      if [[ "$character" == "'" || "$character" == '"' ]]; then
        if [[ "$quote" == "$character" ]]; then
          quote=""
        elif [[ -z "$quote" ]]; then
          quote="$character"
        fi
        saw_non_whitespace=1
        continue
      fi
      if [[ -n "$quote" ]]; then
        saw_non_whitespace=1
        continue
      fi
      if [[ "$character" == [[:space:]] ]]; then
        if (( saw_non_whitespace )); then
          REPLY="${value[1,$index]}"
          return 0
        fi
        continue
      fi
      saw_non_whitespace=1
      if [[ "$character" == '[' ]]; then
        (( bracket_depth++ ))
      elif [[ "$character" == ']' ]]; then
        (( bracket_depth > 0 )) && (( bracket_depth-- ))
      elif (( bracket_depth == 0 )); then
        if [[ "$character" == '@' || "$character" == '=' || "$character" == ',' ]]; then
          REPLY="${value[1,$index]}"
          return 0
        fi
        if [[ "$character" == ':' ]]; then
          boundary=$index
          while (( boundary < ${#value} )); do
            next="${value[$(( boundary + 1 ))]}"
            [[ "$next" == ':' || "$next" == '/' ]] || break
            (( boundary++ ))
          done
          REPLY="${value[1,$boundary]}"
          return 0
        fi
        if [[ "$character" == '/' ]]; then
          boundary=$index
          while (( boundary < ${#value} )) && [[ "${value[$(( boundary + 1 ))]}" == '/' ]]; do
            (( boundary++ ))
          done
          REPLY="${value[1,$boundary]}"
          return 0
        fi
      fi
    done
  }

  _zsuggestion_menu_accept() {
    local mode="${1:-candidate}"
    local accept="${_ZSUGGESTION_MENU_ACCEPTS[$_ZSUGGESTION_MENU_INDEX]}"
    local display="${_ZSUGGESTION_MENU_DISPLAYS[$_ZSUGGESTION_MENU_INDEX]}"
    local source="${_ZSUGGESTION_MENU_SOURCES[$_ZSUGGESTION_MENU_INDEX]}"
    local kind="${_ZSUGGESTION_MENU_KINDS[$_ZSUGGESTION_MENU_INDEX]}"
    (( CURSOR == ${#BUFFER} )) || return 1
    if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      _zsuggestion_menu_clear
      BUFFER="$accept"
      [[ "$source" == command && "$BUFFER" != *[[:space:]]* ]] && BUFFER+=" "
      CURSOR=${#BUFFER}
      _ZSUGGESTION_MENU_BUFFER="$BUFFER"
      _zsuggestion_menu_schedule
      return 0
    fi
    if [[ "$mode" == segment ]]; then
      _zsuggestion_next_segment "$accept"
      accept="$REPLY"
    fi
    LBUFFER+="$accept"
    if [[ ( "$source" == native || "$kind" == file || "$kind" == option ||
            "$kind" == subcommand || "$kind" == value ) &&
          "$BUFFER" == "$display" &&
          "${BUFFER[-1]}" != [@/:=,[:space:]] ]]; then
      LBUFFER+=" "
    fi
    POSTDISPLAY=""
    if [[ "$mode" == segment ]]; then
      _ZSUGGESTION_MENU_INDEX=1
      _ZSUGGESTION_MENU_START=1
      _ZSUGGESTION_MENU_RESTORE_INDEX=1
      _zsuggestion_menu_refresh
    else
      _zsuggestion_menu_clear
      _ZSUGGESTION_MENU_BUFFER="$BUFFER"
      _zsuggestion_menu_schedule
    fi
  }

  _zsuggestion_path_accept() {
    local index found=0 common="" candidate
    for (( index = 1; index <= ${#_ZSUGGESTION_MENU_ACCEPTS}; index++ )); do
      [[ "${_ZSUGGESTION_MENU_KINDS[$index]}" == file ||
         "${_ZSUGGESTION_MENU_KINDS[$index]}" == directory ]] || continue
      (( found++ ))
      candidate="${_ZSUGGESTION_MENU_ACCEPTS[$index]}"
      if [[ -z "$common" ]]; then
        common="$candidate"
      else
        while [[ -n "$common" && "$candidate" != "$common"* ]]; do
          common="${common[1,-2]}"
        done
      fi
    done
    if [[ "${_ZSUGGESTION_MENU_KINDS[$_ZSUGGESTION_MENU_INDEX]}" == file &&
          "${_ZSUGGESTION_MENU_DISPLAYS[$_ZSUGGESTION_MENU_INDEX]}" == "$BUFFER" ]]; then
      _zsuggestion_menu_accept segment
      return
    fi
    if (( found == 1 )) && [[ "${_ZSUGGESTION_MENU_DESCRIPTIONS[$_ZSUGGESTION_MENU_INDEX]}" != *"(more matches)" ]]; then
      _zsuggestion_menu_accept segment
      return
    fi
    if [[ -n "$common" && "${(j: :)_ZSUGGESTION_MENU_DESCRIPTIONS}" != *"(more matches)"* ]]; then
      _zsuggestion_next_segment "$common"
      if [[ -n "$REPLY" ]]; then
        LBUFFER+="$REPLY"
        POSTDISPLAY=""
        _ZSUGGESTION_MENU_INDEX=1
        _ZSUGGESTION_MENU_START=1
        _ZSUGGESTION_MENU_RESTORE_INDEX=1
        _zsuggestion_menu_refresh
        return
      fi
    fi
    zle beep
  }

  _zsuggestion_call_native_completion() {
    local widget="$1"
    _ZSUGGESTION_IN_NATIVE_COMPLETION=1
    {
      zle "$widget"
    } always {
      _ZSUGGESTION_IN_NATIVE_COMPLETION=0
    }
  }

  _zsuggestion_menu_refresh() {
    local previous_buffer="$_ZSUGGESTION_MENU_BUFFER" delta="" accept
    local index retained_index=$_ZSUGGESTION_MENU_INDEX
    local -a accepts displays descriptions kinds sources
    if (( _ZSUGGESTION_MENU_ACTIVE )) && [[ "$BUFFER" == "$previous_buffer"* ]]; then
      delta="${BUFFER[${#previous_buffer}+1,-1]}"
      for (( index = 1; index <= ${#_ZSUGGESTION_MENU_DISPLAYS}; index++ )); do
        [[ "${_ZSUGGESTION_MENU_DISPLAYS[$index]}" == "$BUFFER"* ]] || continue
        accept="${_ZSUGGESTION_MENU_ACCEPTS[$index]}"
        if [[ -n "$delta" ]]; then
          [[ "$accept" == "$delta"* ]] || continue
          accept="${accept[${#delta}+1,-1]}"
        fi
        if [[ -z "$accept" ]]; then
          if [[ "${_ZSUGGESTION_MENU_KINDS[$index]}" == file &&
                "${_ZSUGGESTION_MENU_DISPLAYS[$index]}" == "$BUFFER" ]]; then
            accept=" "
          else
            continue
          fi
        fi
        accepts+=("$accept")
        displays+=("${_ZSUGGESTION_MENU_DISPLAYS[$index]}")
        descriptions+=("${_ZSUGGESTION_MENU_DESCRIPTIONS[$index]}")
        kinds+=("${_ZSUGGESTION_MENU_KINDS[$index]}")
        sources+=("${_ZSUGGESTION_MENU_SOURCES[$index]}")
      done
    fi
    if (( ${#displays} )); then
      _ZSUGGESTION_MENU_ACCEPTS=("${accepts[@]}")
      _ZSUGGESTION_MENU_DISPLAYS=("${displays[@]}")
      _ZSUGGESTION_MENU_DESCRIPTIONS=("${descriptions[@]}")
      _ZSUGGESTION_MENU_KINDS=("${kinds[@]}")
      _ZSUGGESTION_MENU_SOURCES=("${sources[@]}")
      (( retained_index > ${#_ZSUGGESTION_MENU_DISPLAYS} )) && retained_index=${#_ZSUGGESTION_MENU_DISPLAYS}
      (( retained_index < 1 )) && retained_index=1
      _ZSUGGESTION_MENU_INDEX=$retained_index
      _ZSUGGESTION_MENU_START=$retained_index
      _ZSUGGESTION_MENU_BUFFER="$BUFFER"
      _ZSUGGESTION_CAPTURE_FOREIGN_HIGHLIGHTS=1
      _zsuggestion_menu_render
    else
      _zsuggestion_menu_clear
    fi
    _ZSUGGESTION_MENU_BUFFER="$BUFFER"
    if [[ -n "$BUFFER" ]] && (( CURSOR == ${#BUFFER} )); then
      _zsuggestion_menu_schedule
    fi
  }

  _zsuggestion_menu_schedule() {
    local LC_ALL=C
    _zsuggestion_menu_cancel_query
    _zsuggestion_native_cancel
    _ZSUGGESTION_MENU_REQUEST_BUFFER="$BUFFER"
    _ZSUGGESTION_MENU_REQUEST_CWD="$PWD"
    _ZSUGGESTION_MENU_REQUEST_CURSOR=${#LBUFFER}
    _ZSUGGESTION_MENU_REQUEST_DIRTY=1
    _ZSUGGESTION_NATIVE_REQUESTED=0
    _ZSUGGESTION_NATIVE_START_TICKS=0
    _ZSUGGESTION_DAEMON_ACCEPTS=()
    _ZSUGGESTION_DAEMON_DISPLAYS=()
    _ZSUGGESTION_DAEMON_DESCRIPTIONS=()
    _ZSUGGESTION_DAEMON_KINDS=()
    _ZSUGGESTION_DAEMON_SOURCES=()
    _ZSUGGESTION_NATIVE_ACCEPTS=()
    _ZSUGGESTION_NATIVE_DISPLAYS=()
    _ZSUGGESTION_NATIVE_DESCRIPTIONS=()
  }

  _zsuggestion_menu_publish() {
    local index display
    local -a redraw_hooks apply_hooks
    local limit=$(( __ZSUGGESTION_COMPLETION_MAX_CANDIDATES__ * 2 ))
    _ZSUGGESTION_MENU_ACCEPTS=()
    _ZSUGGESTION_MENU_DISPLAYS=()
    _ZSUGGESTION_MENU_DESCRIPTIONS=()
    _ZSUGGESTION_MENU_KINDS=()
    _ZSUGGESTION_MENU_SOURCES=()

    for (( index = 1; index <= ${#_ZSUGGESTION_DAEMON_ACCEPTS}; index++ )); do
      display="${_ZSUGGESTION_DAEMON_DISPLAYS[$index]}"
      [[ -n "$display" ]] && (( ! ${_ZSUGGESTION_MENU_DISPLAYS[(Ie)$display]} )) || continue
      _ZSUGGESTION_MENU_ACCEPTS+=("${_ZSUGGESTION_DAEMON_ACCEPTS[$index]}")
      _ZSUGGESTION_MENU_DISPLAYS+=("$display")
      _ZSUGGESTION_MENU_DESCRIPTIONS+=("${_ZSUGGESTION_DAEMON_DESCRIPTIONS[$index]}")
      _ZSUGGESTION_MENU_KINDS+=("${_ZSUGGESTION_DAEMON_KINDS[$index]}")
      _ZSUGGESTION_MENU_SOURCES+=("${_ZSUGGESTION_DAEMON_SOURCES[$index]}")
      (( ${#_ZSUGGESTION_MENU_ACCEPTS} >= limit )) && break
    done

    if (( ${#_ZSUGGESTION_MENU_ACCEPTS} < limit )); then
      for (( index = 1; index <= ${#_ZSUGGESTION_NATIVE_ACCEPTS}; index++ )); do
        display="${_ZSUGGESTION_NATIVE_DISPLAYS[$index]}"
        [[ -n "$display" ]] && (( ! ${_ZSUGGESTION_MENU_DISPLAYS[(Ie)$display]} )) || continue
        _ZSUGGESTION_MENU_ACCEPTS+=("${_ZSUGGESTION_NATIVE_ACCEPTS[$index]}")
        _ZSUGGESTION_MENU_DISPLAYS+=("$display")
        _ZSUGGESTION_MENU_DESCRIPTIONS+=("${_ZSUGGESTION_NATIVE_DESCRIPTIONS[$index]}")
        _ZSUGGESTION_MENU_KINDS+=("native")
        _ZSUGGESTION_MENU_SOURCES+=("native")
        (( ${#_ZSUGGESTION_MENU_ACCEPTS} >= limit )) && break
      done
    fi

    _ZSUGGESTION_MENU_RESTORE_INDEX=$_ZSUGGESTION_MENU_INDEX
    zstyle -a zle-line-pre-redraw widgets redraw_hooks
    apply_hooks=( "${(@)redraw_hooks:#*:_zsh_highlight__zle-line-pre-redraw}" )
    if (( ${#apply_hooks} != ${#redraw_hooks} )); then
      zstyle zle-line-pre-redraw widgets "${apply_hooks[@]}"
      {
        zle zsuggestion-menu-apply
      } always {
        zstyle zle-line-pre-redraw widgets "${redraw_hooks[@]}"
      }
    else
      zle zsuggestion-menu-apply
    fi
    region_highlight=( "${_ZSUGGESTION_RENDERED_HIGHLIGHTS[@]}" )
  }

  _zsuggestion_menu_request_ready() {
    local fd="$1" accept display description kind source pending response_pending
    local any_pending=0
    local -a accepts displays descriptions kinds sources
    if (( fd != _ZSUGGESTION_MENU_REQUEST_FD )); then
      zle -F "$fd" 2>/dev/null
      exec {fd}<&-
      return 0
    fi
    if [[ "$_ZSUGGESTION_MENU_BUFFER" != "$_ZSUGGESTION_MENU_INFLIGHT_BUFFER" ]] ||
       (( _ZSUGGESTION_MENU_INFLIGHT_GENERATION != _ZSUGGESTION_MENU_GENERATION )); then
      zle -F "$fd" 2>/dev/null
      exec {fd}<&-
      _ZSUGGESTION_MENU_REQUEST_FD=-1
      _ZSUGGESTION_MENU_INFLIGHT_BUFFER=""
      _ZSUGGESTION_MENU_INFLIGHT_CURSOR=0
      return 0
    fi

    zle -F "$fd"
    BUFFER="$_ZSUGGESTION_MENU_INFLIGHT_BUFFER"
    CURSOR=$_ZSUGGESTION_MENU_INFLIGHT_CURSOR
    if IFS= read -r -u "$fd" -d '' response_pending && [[ "$response_pending" == true ]]; then
      any_pending=1
    fi
    while IFS= read -r -u "$fd" -d '' accept &&
          IFS= read -r -u "$fd" -d '' display &&
          IFS= read -r -u "$fd" -d '' description &&
          IFS= read -r -u "$fd" -d '' kind &&
          IFS= read -r -u "$fd" -d '' source &&
          IFS= read -r -u "$fd" -d '' pending; do
      accepts+=("$accept")
      displays+=("$display")
      descriptions+=("$description")
      kinds+=("$kind")
      sources+=("$source")
      [[ "$pending" == true ]] && any_pending=1
    done
    exec {fd}<&-

    _ZSUGGESTION_MENU_REQUEST_FD=-1
    _ZSUGGESTION_DAEMON_ACCEPTS=("${accepts[@]}")
    _ZSUGGESTION_DAEMON_DISPLAYS=("${displays[@]}")
    _ZSUGGESTION_DAEMON_DESCRIPTIONS=("${descriptions[@]}")
    _ZSUGGESTION_DAEMON_KINDS=("${kinds[@]}")
    _ZSUGGESTION_DAEMON_SOURCES=("${sources[@]}")
    (( any_pending )) && _ZSUGGESTION_MENU_REFRESH_TICKS=5 || _ZSUGGESTION_MENU_REFRESH_TICKS=0
    if (( ! _ZSUGGESTION_FUZZY_ACTIVE && ${#accepts} == 0 &&
          ! _ZSUGGESTION_MENU_ACTIVE )); then
      _ZSUGGESTION_NATIVE_ACCEPTS=()
      _ZSUGGESTION_NATIVE_DISPLAYS=()
      _ZSUGGESTION_NATIVE_DESCRIPTIONS=()
      return 0
    fi
    if (( ! _ZSUGGESTION_FUZZY_ACTIVE && ${#accepts} == 0 && _ZSUGGESTION_MENU_ACTIVE &&
          $+functions[_main_complete] )) &&
       (( ! _ZSUGGESTION_NATIVE_REQUESTED || _ZSUGGESTION_NATIVE_REQUEST_FD >= 0 )); then
      return 0
    fi
    _zsuggestion_menu_publish
  }

  _zsuggestion_native_request_ready() {
    local fd="$1" accept display description
    local -a accepts displays descriptions
    if (( fd != _ZSUGGESTION_NATIVE_REQUEST_FD )); then
      zle -F "$fd" 2>/dev/null
      exec {fd}<&-
      return 0
    fi
    if [[ "$_ZSUGGESTION_MENU_BUFFER" != "$_ZSUGGESTION_NATIVE_INFLIGHT_BUFFER" ]] ||
       (( _ZSUGGESTION_NATIVE_INFLIGHT_GENERATION != _ZSUGGESTION_MENU_GENERATION )); then
      zle -F "$fd" 2>/dev/null
      exec {fd}<&-
      _ZSUGGESTION_NATIVE_REQUEST_FD=-1
      _ZSUGGESTION_NATIVE_REQUEST_PID=-1
      _ZSUGGESTION_NATIVE_REQUEST_TICKS=0
      _ZSUGGESTION_NATIVE_INFLIGHT_BUFFER=""
      _ZSUGGESTION_NATIVE_INFLIGHT_CURSOR=0
      return 0
    fi

    zle -F "$fd"
    BUFFER="$_ZSUGGESTION_NATIVE_INFLIGHT_BUFFER"
    CURSOR=$_ZSUGGESTION_NATIVE_INFLIGHT_CURSOR
    while IFS= read -r -u "$fd" -d '' accept &&
          IFS= read -r -u "$fd" -d '' display &&
          IFS= read -r -u "$fd" -d '' description; do
      accepts+=("$accept")
      displays+=("$display")
      descriptions+=("$description")
    done
    exec {fd}<&-
    _ZSUGGESTION_NATIVE_REQUEST_FD=-1
    _ZSUGGESTION_NATIVE_REQUEST_PID=-1
    _ZSUGGESTION_NATIVE_REQUEST_TICKS=0

    _ZSUGGESTION_NATIVE_ACCEPTS=("${accepts[@]}")
    _ZSUGGESTION_NATIVE_DISPLAYS=("${displays[@]}")
    _ZSUGGESTION_NATIVE_DESCRIPTIONS=("${descriptions[@]}")
    _zsuggestion_menu_publish
  }

  _zsuggestion_native_capture_widget() {
    local fd
    exec {fd}< <(
      local original_lbuffer="$LBUFFER"
      local -a accepts displays descriptions

      compadd() {
        local argument status current_prefix="$PREFIX"
        local added_prefix="" hidden_prefix="" ignored_prefix=""
        local added_suffix="" hidden_suffix="" ignored_suffix=""
        local index=1
        for argument in "$@"; do
          if [[ "$argument" == -A || "$argument" == -O || "$argument" == -D ]]; then
            builtin compadd "$@"
            return
          fi
        done
        while (( index <= $# )); do
          argument="${@[index]}"
          case "$argument" in
            --) break ;;
            -P|-p|-i|-S|-s|-I)
              (( index++ ))
              (( index <= $# )) || break
              case "$argument" in
                -P) added_prefix="${@[index]}" ;;
                -p) hidden_prefix="${@[index]}" ;;
                -i) ignored_prefix="${@[index]}" ;;
                -S) added_suffix="${@[index]}" ;;
                -s) hidden_suffix="${@[index]}" ;;
                -I) ignored_suffix="${@[index]}" ;;
              esac
              ;;
          esac
          (( index++ ))
        done

        local -a generated
        generated=("${(@0)$(
          local -a matches
          builtin compadd -A matches "$@" 2>/dev/null
          local match
          for match in "${matches[@]}"; do
            print -rn -- "$match"$'\0'
          done
        )}")

        if [[ -z "$IPREFIX$ISUFFIX$QIPREFIX$QISUFFIX$SUFFIX$added_prefix$hidden_prefix$ignored_prefix" ]]; then
          local base_length=$(( ${#original_lbuffer} - ${#current_prefix} ))
          local base="" match full accept
          (( base_length > 0 )) && base="${original_lbuffer[1,$base_length]}"
          if (( base_length >= 0 )) &&
             [[ -z "$current_prefix" ||
                "${original_lbuffer[$(( base_length + 1 )),-1]}" == "$current_prefix" ]]; then
            for match in "${generated[@]}"; do
              (( ${#displays} < __ZSUGGESTION_COMPLETION_MAX_CANDIDATES__ )) || break
              [[ -n "$match" && "${(q)match}" == "$match" ]] || continue
              [[ -z "$current_prefix" ||
                 "${match[1,${#current_prefix}]}" == "$current_prefix" ]] || continue
              full="${base}${match}${hidden_suffix}${added_suffix}${ignored_suffix}"
              [[ "$full" == "$original_lbuffer"* && "$full" != "$original_lbuffer" ]] || continue
              (( ${displays[(Ie)$full]} )) && continue
              accept="${full[${#original_lbuffer}+1,-1]}"
              [[ -n "$accept" ]] || continue
              accepts+=("$accept")
              displays+=("$full")
              descriptions+=("Zsh completion")
            done
          fi
        fi

        builtin compadd "$@"
        status=$?
        return status
      }

      _main_complete >/dev/null 2>&1
      local index
      for (( index = 1; index <= ${#accepts}; index++ )); do
        printf '%s\0%s\0%s\0' \
          "${accepts[$index]}" \
          "${displays[$index]}" \
          "${descriptions[$index]}"
      done
    )
    _ZSUGGESTION_NATIVE_REQUEST_FD="$fd"
    _ZSUGGESTION_NATIVE_REQUEST_PID=$!
    _ZSUGGESTION_NATIVE_REQUEST_TICKS=0
    zle -F "$fd" _zsuggestion_native_request_ready
    compstate[insert]=''
    compstate[list]=''
  }

  _zsuggestion_menu_apply_result() {
    local -a rendered_highlights
    if (( ${#_ZSUGGESTION_MENU_ACCEPTS} == 0 )); then
      _ZSUGGESTION_MENU_ACTIVE=0
      region_highlight=( "${_ZSUGGESTION_FOREIGN_HIGHLIGHTS[@]}" )
      if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
        _zsuggestion_menu_render
      else
        POSTDISPLAY=""
      fi
      rendered_highlights=( "${region_highlight[@]}" )
      _ZSUGGESTION_RENDERED_HIGHLIGHTS=( "${rendered_highlights[@]}" )
      _ZSUGGESTION_RESTORE_HIGHLIGHTS=1
      zle -R
      region_highlight=( "${rendered_highlights[@]}" )
      return 0
    fi
    _ZSUGGESTION_MENU_ACTIVE=1
    if (( _ZSUGGESTION_MENU_KEYTIMEOUT < 0 )); then
      _ZSUGGESTION_MENU_KEYTIMEOUT=$KEYTIMEOUT
    fi
    (( KEYTIMEOUT > 2 )) && KEYTIMEOUT=2
    _ZSUGGESTION_MENU_INDEX=$_ZSUGGESTION_MENU_RESTORE_INDEX
    (( _ZSUGGESTION_MENU_INDEX > ${#_ZSUGGESTION_MENU_DISPLAYS} )) && _ZSUGGESTION_MENU_INDEX=${#_ZSUGGESTION_MENU_DISPLAYS}
    (( _ZSUGGESTION_MENU_INDEX < 1 )) && _ZSUGGESTION_MENU_INDEX=1
    _ZSUGGESTION_MENU_RESTORE_INDEX=1
    _ZSUGGESTION_MENU_START=$_ZSUGGESTION_MENU_INDEX
    _ZSUGGESTION_MENU_BUFFER="$_ZSUGGESTION_MENU_REQUEST_BUFFER"
    region_highlight=( "${_ZSUGGESTION_FOREIGN_HIGHLIGHTS[@]}" )
    _zsuggestion_menu_render
    rendered_highlights=( "${region_highlight[@]}" )
    _ZSUGGESTION_RENDERED_HIGHLIGHTS=( "${rendered_highlights[@]}" )
    _ZSUGGESTION_RESTORE_HIGHLIGHTS=1
    zle -R
    region_highlight=( "${rendered_highlights[@]}" )
  }

  _zsuggestion_menu_start_ticker() {
    (( _ZSUGGESTION_MENU_TICK_FD >= 0 )) && return 0
    local fd
    exec {fd}< <(
      if (( _ZSUGGESTION_HAS_ZSELECT )); then
        while true; do
          zselect -t 3
          print -r -- tick || exit
        done
      else
        while sleep 0.03; do
          print -r -- tick || exit
        done
      fi
    )
    _ZSUGGESTION_MENU_TICK_FD="$fd"
    zle -F "$fd" _zsuggestion_menu_tick
  }

  _zsuggestion_menu_stop_ticker() {
    local fd="$_ZSUGGESTION_MENU_TICK_FD"
    if (( fd >= 0 )); then
      zle -F "$fd" 2>/dev/null
      exec {fd}<&-
      _ZSUGGESTION_MENU_TICK_FD=-1
    fi
  }

  _zsuggestion_menu_tick() {
    local fd="$1" tick query_fd buffer cwd cursor
    if (( fd != _ZSUGGESTION_MENU_TICK_FD )) || ! IFS= read -r -u "$fd" tick; then
      zle -F "$fd" 2>/dev/null
      exec {fd}<&-
      _ZSUGGESTION_MENU_TICK_FD=-1
      return 0
    fi
    (( _ZSUGGESTION_IN_NATIVE_COMPLETION )) && return 0
    if [[ -n "$_ZSUGGESTION_PREVIEW_PATH" || -n "$_ZSUGGESTION_PREVIEW_COMMAND" ]]; then
      (( _ZSUGGESTION_PREVIEW_TICKS++ ))
      if (( _ZSUGGESTION_PREVIEW_FD < 0 && _ZSUGGESTION_PREVIEW_TICKS >= 2 )); then
        local preview_fd preview_target="$_ZSUGGESTION_PREVIEW_TARGET"
        exec {preview_fd}< <(
          print -rn -- "$preview_target"$'\0'
          if [[ -n "$_ZSUGGESTION_PREVIEW_COMMAND" ]]; then
            command zsuggestion preview-command \
              --line "$_ZSUGGESTION_PREVIEW_COMMAND" \
              --cwd "$PWD" 2>/dev/null
          else
            command zsuggestion preview-file \
              --path "$_ZSUGGESTION_PREVIEW_PATH" \
              --cwd "$PWD" 2>/dev/null
          fi
        )
        _ZSUGGESTION_PREVIEW_FD="$preview_fd"
        _ZSUGGESTION_PREVIEW_PID=$!
        _ZSUGGESTION_PREVIEW_TICKS=0
        zle -F "$preview_fd" _zsuggestion_preview_ready
      elif (( _ZSUGGESTION_PREVIEW_FD >= 0 && _ZSUGGESTION_PREVIEW_TICKS > 34 )); then
        _zsuggestion_preview_cancel
        _ZSUGGESTION_PREVIEW_PATH=""
        _ZSUGGESTION_PREVIEW_COMMAND=""
        _ZSUGGESTION_PREVIEW_LINES=("Preview timed out")
        _ZSUGGESTION_PREVIEW_STYLES=("")
        zle -R
      fi
    fi
    if (( _ZSUGGESTION_NATIVE_REQUEST_FD >= 0 )); then
      (( _ZSUGGESTION_NATIVE_REQUEST_TICKS++ ))
      if (( _ZSUGGESTION_NATIVE_REQUEST_TICKS > 50 )); then
        _zsuggestion_native_cancel
        _zsuggestion_menu_publish
      fi
    fi
    if (( ! _ZSUGGESTION_FUZZY_ACTIVE && ! _ZSUGGESTION_NATIVE_REQUESTED )) &&
       [[ -n "$_ZSUGGESTION_MENU_REQUEST_BUFFER" ]]; then
      (( _ZSUGGESTION_NATIVE_START_TICKS++ ))
      if (( _ZSUGGESTION_NATIVE_START_TICKS >= 2 )) && (( $+functions[_main_complete] )); then
        _ZSUGGESTION_NATIVE_REQUESTED=1
        _ZSUGGESTION_NATIVE_INFLIGHT_BUFFER="$_ZSUGGESTION_MENU_REQUEST_BUFFER"
        _ZSUGGESTION_NATIVE_INFLIGHT_CURSOR=$_ZSUGGESTION_MENU_REQUEST_CURSOR
        _ZSUGGESTION_NATIVE_INFLIGHT_GENERATION=$_ZSUGGESTION_MENU_GENERATION
        _zsuggestion_call_native_completion zsuggestion-native-capture
      fi
    fi
    if (( ! _ZSUGGESTION_MENU_REQUEST_DIRTY )); then
      (( _ZSUGGESTION_MENU_REFRESH_TICKS > 0 )) || return 0
      (( _ZSUGGESTION_MENU_REFRESH_TICKS-- ))
      (( _ZSUGGESTION_MENU_REFRESH_TICKS == 0 )) || return 0
      _ZSUGGESTION_MENU_REQUEST_DIRTY=1
    fi

    _ZSUGGESTION_MENU_REQUEST_DIRTY=0
    buffer="$_ZSUGGESTION_MENU_REQUEST_BUFFER"
    cwd="$_ZSUGGESTION_MENU_REQUEST_CWD"
    cursor="$_ZSUGGESTION_MENU_REQUEST_CURSOR"
    if [[ -z "$buffer" ]]; then
      return 0
    fi

    _zsuggestion_menu_cancel_query
    _ZSUGGESTION_MENU_INFLIGHT_BUFFER="$buffer"
    _ZSUGGESTION_MENU_INFLIGHT_CURSOR=$cursor
    _ZSUGGESTION_MENU_INFLIGHT_GENERATION=$_ZSUGGESTION_MENU_GENERATION
    if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      exec {query_fd}< <(command zsuggestion fuzzy \
        --query "$_ZSUGGESTION_FUZZY_QUERY" \
        --cwd "$cwd" \
        --format zsh-v3 2>/dev/null)
    else
      exec {query_fd}< <(print -rn -- "$buffer" | command zsuggestion complete \
        --stdin \
        --cursor "$cursor" \
        --cwd "$cwd" \
        --format zsh-v3 2>/dev/null)
    fi
    _ZSUGGESTION_MENU_REQUEST_FD="$query_fd"
    zle -F "$query_fd" _zsuggestion_menu_request_ready
  }

  _zsuggestion_menu_line_init() {
    _ZSUGGESTION_FUZZY_ACTIVE=0
    _ZSUGGESTION_FUZZY_BASE=""
    _ZSUGGESTION_FUZZY_QUERY=""
    POSTDISPLAY=""
    _ZSUGGESTION_MENU_BUFFER="$BUFFER"
    _zsuggestion_menu_start_ticker
  }

  _zsuggestion_menu_line_finish() {
    _zsuggestion_menu_clear
    _zsuggestion_menu_stop_ticker
  }

  _zsuggestion_self_insert() {
    if (( _ZSUGGESTION_IN_BRACKETED_PASTE )); then
      zle _zsuggestion-native-self-insert
      return
    fi
    if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      _ZSUGGESTION_FUZZY_QUERY+="$KEYS"
      _zsuggestion_fuzzy_refresh
      return
    fi
    zle _zsuggestion-native-self-insert
    _zsuggestion_menu_refresh
  }

  _zsuggestion_space() {
    if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      _ZSUGGESTION_FUZZY_QUERY+=" "
      _zsuggestion_fuzzy_refresh
      return
    fi
    if [[ "$LBUFFER" == *" " ]] && (( CURSOR == ${#BUFFER} )); then
      LBUFFER="${LBUFFER% }"
      _zsuggestion_fuzzy_start
      return
    fi
    zle _zsuggestion-native-space
    _zsuggestion_menu_refresh
  }

  _zsuggestion_backward_delete() {
    if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      if [[ -n "$_ZSUGGESTION_FUZZY_QUERY" ]]; then
        _ZSUGGESTION_FUZZY_QUERY="${_ZSUGGESTION_FUZZY_QUERY[1,-2]}"
        _zsuggestion_fuzzy_refresh
      else
        _zsuggestion_menu_clear
        zle _zsuggestion-native-backward-delete
        _zsuggestion_menu_refresh
      fi
      return
    fi
    zle _zsuggestion-native-backward-delete
    _zsuggestion_menu_refresh
  }

  _zsuggestion_bracketed_paste() {
    _zsuggestion_menu_clear 1
    POSTDISPLAY=""
    _ZSUGGESTION_IN_BRACKETED_PASTE=1
    {
      zle _zsuggestion-native-bracketed-paste
    } always {
      _ZSUGGESTION_IN_BRACKETED_PASTE=0
    }
    _zsuggestion_menu_refresh
  }

  _zsuggestion_interrupt() {
    _zsuggestion_menu_clear 1
    POSTDISPLAY=""
    _ZSUGGESTION_FUZZY_ACTIVE=0
    _ZSUGGESTION_FUZZY_BASE=""
    _ZSUGGESTION_FUZZY_QUERY=""
    _ZSUGGESTION_MENU_REQUEST_BUFFER=""
    zle _zsuggestion-native-interrupt
  }

  _zsuggestion_tab() {
    if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      if (( _ZSUGGESTION_MENU_ACTIVE )); then
        _zsuggestion_menu_accept
      else
        zle beep
      fi
      return
    fi
    if (( _ZSUGGESTION_MENU_ACTIVE && CURSOR == ${#BUFFER} )); then
      if [[ "${_ZSUGGESTION_MENU_KINDS[$_ZSUGGESTION_MENU_INDEX]}" == file ||
            "${_ZSUGGESTION_MENU_KINDS[$_ZSUGGESTION_MENU_INDEX]}" == directory ]]; then
        _zsuggestion_path_accept
        return
      fi
      _zsuggestion_menu_accept segment
      return
    fi
    _zsuggestion_menu_clear
    POSTDISPLAY=""
    _zsuggestion_call_native_completion _zsuggestion-native-tab
    _ZSUGGESTION_MENU_BUFFER="$BUFFER"
  }

  _zsuggestion_shift_tab() {
    if (( _ZSUGGESTION_MENU_ACTIVE )); then
      (( _ZSUGGESTION_MENU_INDEX > 1 )) && (( _ZSUGGESTION_MENU_INDEX-- ))
      _zsuggestion_menu_render
      zle -R
      return
    fi
    if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      zle beep
      return
    fi
    _zsuggestion_menu_clear
    POSTDISPLAY=""
    _zsuggestion_call_native_completion _zsuggestion-native-shift-tab
    _ZSUGGESTION_MENU_BUFFER="$BUFFER"
  }

  _zsuggestion_complete() {
    if (( _ZSUGGESTION_FUZZY_ACTIVE && ! _ZSUGGESTION_MENU_ACTIVE )); then
      zle beep
      return
    fi
    if (( _ZSUGGESTION_MENU_ACTIVE && CURSOR == ${#BUFFER} )); then
      _zsuggestion_menu_accept
    elif (( CURSOR == ${#BUFFER} )) && _zsuggestion_menu_query; then
      _zsuggestion_menu_accept
    else
      (( _ZSUGGESTION_MENU_ACTIVE )) && _zsuggestion_menu_clear 1
      _zsuggestion_call_native_completion _zsuggestion-native-trigger
    fi
  }

  _zsuggestion_fuzzy_execute() {
    if (( ! _ZSUGGESTION_FUZZY_ACTIVE || ! _ZSUGGESTION_MENU_ACTIVE )); then
      zle beep
      return
    fi
    _zsuggestion_menu_accept || return
    zle _zsuggestion-native-enter
  }

  _zsuggestion_menu_down() {
    if (( _ZSUGGESTION_MENU_ACTIVE )); then
      (( _ZSUGGESTION_MENU_INDEX < ${#_ZSUGGESTION_MENU_ACCEPTS} )) && (( _ZSUGGESTION_MENU_INDEX++ ))
      _zsuggestion_menu_render
      zle -R
    elif (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      zle beep
    else
      zle _zsuggestion-native-down
      _zsuggestion_menu_refresh
    fi
  }

  _zsuggestion_menu_up() {
    if (( _ZSUGGESTION_MENU_ACTIVE )); then
      (( _ZSUGGESTION_MENU_INDEX > 1 )) && (( _ZSUGGESTION_MENU_INDEX-- ))
      _zsuggestion_menu_render
      zle -R
    elif (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      zle beep
    else
      zle _zsuggestion-native-up
      _zsuggestion_menu_refresh
    fi
  }

  _zsuggestion_escape() {
    if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      local base="$_ZSUGGESTION_FUZZY_BASE"
      _zsuggestion_menu_clear
      BUFFER="$base"
      CURSOR=${#BUFFER}
      _ZSUGGESTION_MENU_BUFFER="$BUFFER"
      [[ "$BUFFER" == *[![:space:]]* ]] && _zsuggestion_menu_schedule
      region_highlight=( "${_ZSUGGESTION_FOREIGN_HIGHLIGHTS[@]}" )
      zle -R
      return
    fi
    if (( _ZSUGGESTION_MENU_ACTIVE )); then
      _zsuggestion_menu_clear 1
      POSTDISPLAY=""
      region_highlight=( "${_ZSUGGESTION_FOREIGN_HIGHLIGHTS[@]}" )
      zle -R
      return
    fi
    zle _zsuggestion-native-escape
  }

  _zsuggestion_history_move() {
    local native_widget="$1"
    _zsuggestion_menu_clear 1
    POSTDISPLAY=""
    zle "$native_widget"
    _ZSUGGESTION_MENU_BUFFER="$BUFFER"
    zle -R
  }

  _zsuggestion_history_up() {
    if (( _ZSUGGESTION_MENU_ACTIVE )); then
      zle zsuggestion-menu-up
      return
    fi
    _zsuggestion_history_move _zsuggestion-native-history-up
  }

  _zsuggestion_history_up_application() {
    if (( _ZSUGGESTION_MENU_ACTIVE )); then
      zle zsuggestion-menu-up
      return
    fi
    _zsuggestion_history_move _zsuggestion-native-history-up-application
  }

  _zsuggestion_history_down() {
    if (( _ZSUGGESTION_MENU_ACTIVE )); then
      zle zsuggestion-menu-down
      return
    fi
    _zsuggestion_history_move _zsuggestion-native-history-down
  }

  _zsuggestion_history_down_application() {
    if (( _ZSUGGESTION_MENU_ACTIVE )); then
      zle zsuggestion-menu-down
      return
    fi
    _zsuggestion_history_move _zsuggestion-native-history-down-application
  }

  _zsuggestion_menu_pre_redraw() {
    if (( _ZSUGGESTION_CAPTURE_FOREIGN_HIGHLIGHTS )); then
      _ZSUGGESTION_FOREIGN_HIGHLIGHTS=( "${(@)region_highlight:#*memo=zsuggestion*}" )
      _ZSUGGESTION_CAPTURE_FOREIGN_HIGHLIGHTS=0
    elif (( _ZSUGGESTION_MENU_ACTIVE || _ZSUGGESTION_FUZZY_ACTIVE )) &&
         [[ "$BUFFER" == "$_ZSUGGESTION_MENU_BUFFER" ]]; then
      region_highlight=( "${_ZSUGGESTION_FOREIGN_HIGHLIGHTS[@]}" )
      _ZSUGGESTION_RESTORE_HIGHLIGHTS=0
    elif (( _ZSUGGESTION_RESTORE_HIGHLIGHTS )); then
      region_highlight=( "${_ZSUGGESTION_FOREIGN_HIGHLIGHTS[@]}" )
      _ZSUGGESTION_RESTORE_HIGHLIGHTS=0
    elif [[ -n "$BUFFER" ]]; then
      _ZSUGGESTION_FOREIGN_HIGHLIGHTS=( "${(@)region_highlight:#*memo=zsuggestion*}" )
    elif [[ -n "$_ZSUGGESTION_MENU_REQUEST_BUFFER" ]]; then
      region_highlight=( "${_ZSUGGESTION_FOREIGN_HIGHLIGHTS[@]}" )
    else
      _ZSUGGESTION_FOREIGN_HIGHLIGHTS=()
    fi
    if [[ "$BUFFER" != "$_ZSUGGESTION_MENU_BUFFER" ]]; then
      _zsuggestion_menu_clear
      _ZSUGGESTION_MENU_BUFFER="$BUFFER"
      [[ -n "$BUFFER" ]] && (( CURSOR == ${#BUFFER} )) && _zsuggestion_menu_schedule
    elif (( CURSOR != ${#BUFFER} )); then
      if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
        _zsuggestion_menu_clear
        _ZSUGGESTION_MENU_BUFFER="$BUFFER"
      else
        _zsuggestion_menu_cancel_request
      fi
    fi
    (( _ZSUGGESTION_MENU_ACTIVE || _ZSUGGESTION_FUZZY_ACTIVE )) && _zsuggestion_menu_render
  }

  autoload -Uz add-zsh-hook
  autoload -Uz add-zle-hook-widget
  zmodload zsh/zselect 2>/dev/null && _ZSUGGESTION_HAS_ZSELECT=1
  if (( $+functions[_zsh_autosuggest_bind_widgets] )); then
    typeset -g ZSH_AUTOSUGGEST_MANUAL_REBIND=1
    precmd_functions=(${precmd_functions:#_zsh_autosuggest_start})
  fi
  add-zsh-hook preexec _zsuggestion_preexec
  add-zsh-hook precmd _zsuggestion_precmd
  add-zle-hook-widget line-init _zsuggestion_menu_line_init
  add-zle-hook-widget line-finish _zsuggestion_menu_line_finish
  add-zle-hook-widget line-pre-redraw _zsuggestion_menu_pre_redraw
  preexec_functions=(_zsuggestion_preexec ${preexec_functions:#_zsuggestion_preexec})
  precmd_functions=(_zsuggestion_precmd ${precmd_functions:#_zsuggestion_precmd})

  if [[ -z "${widgets[_zsuggestion-native-self-insert]:-}" ]]; then
    typeset -g _ZSUGGESTION_PREVIOUS_TRIGGER="${$(bindkey '__ZSUGGESTION_COMPLETION_KEY__')##* }"
    [[ -z "$_ZSUGGESTION_PREVIOUS_TRIGGER" || "$_ZSUGGESTION_PREVIOUS_TRIGGER" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_TRIGGER=set-mark-command
    typeset -g _ZSUGGESTION_PREVIOUS_DOWN="${$(bindkey '^N')##* }"
    typeset -g _ZSUGGESTION_PREVIOUS_UP="${$(bindkey '^K')##* }"
    typeset -g _ZSUGGESTION_PREVIOUS_SHIFT_TAB="${$(bindkey '^[[Z')##* }"
    typeset -g _ZSUGGESTION_PREVIOUS_ESCAPE="${$(bindkey '^[')##* }"
    typeset -g _ZSUGGESTION_PREVIOUS_ENTER="${$(bindkey '^M')##* }"
    typeset -g _ZSUGGESTION_PREVIOUS_HISTORY_UP="${$(bindkey '^[[A')##* }"
    typeset -g _ZSUGGESTION_PREVIOUS_HISTORY_UP_APPLICATION="${$(bindkey '^[OA')##* }"
    typeset -g _ZSUGGESTION_PREVIOUS_HISTORY_DOWN="${$(bindkey '^[[B')##* }"
    typeset -g _ZSUGGESTION_PREVIOUS_HISTORY_DOWN_APPLICATION="${$(bindkey '^[OB')##* }"
    [[ -z "$_ZSUGGESTION_PREVIOUS_DOWN" || "$_ZSUGGESTION_PREVIOUS_DOWN" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_DOWN=down-line-or-history
    [[ -z "$_ZSUGGESTION_PREVIOUS_UP" || "$_ZSUGGESTION_PREVIOUS_UP" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_UP=kill-line
    [[ -z "$_ZSUGGESTION_PREVIOUS_SHIFT_TAB" || "$_ZSUGGESTION_PREVIOUS_SHIFT_TAB" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_SHIFT_TAB=reverse-menu-complete
    [[ -z "$_ZSUGGESTION_PREVIOUS_ESCAPE" ]] && _ZSUGGESTION_PREVIOUS_ESCAPE=undefined-key
    [[ -z "$_ZSUGGESTION_PREVIOUS_ENTER" || "$_ZSUGGESTION_PREVIOUS_ENTER" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_ENTER=accept-line
    [[ -z "$_ZSUGGESTION_PREVIOUS_HISTORY_UP" || "$_ZSUGGESTION_PREVIOUS_HISTORY_UP" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_HISTORY_UP=up-line-or-history
    [[ -z "$_ZSUGGESTION_PREVIOUS_HISTORY_UP_APPLICATION" ||
       "$_ZSUGGESTION_PREVIOUS_HISTORY_UP_APPLICATION" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_HISTORY_UP_APPLICATION=$_ZSUGGESTION_PREVIOUS_HISTORY_UP
    [[ -z "$_ZSUGGESTION_PREVIOUS_HISTORY_DOWN" || "$_ZSUGGESTION_PREVIOUS_HISTORY_DOWN" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_HISTORY_DOWN=down-line-or-history
    [[ -z "$_ZSUGGESTION_PREVIOUS_HISTORY_DOWN_APPLICATION" ||
       "$_ZSUGGESTION_PREVIOUS_HISTORY_DOWN_APPLICATION" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_HISTORY_DOWN_APPLICATION=$_ZSUGGESTION_PREVIOUS_HISTORY_DOWN
    zle -A "$_ZSUGGESTION_PREVIOUS_TRIGGER" _zsuggestion-native-trigger
    zle -A "$_ZSUGGESTION_PREVIOUS_DOWN" _zsuggestion-native-down
    zle -A "$_ZSUGGESTION_PREVIOUS_UP" _zsuggestion-native-up
    zle -A "$_ZSUGGESTION_PREVIOUS_SHIFT_TAB" _zsuggestion-native-shift-tab
    zle -A "$_ZSUGGESTION_PREVIOUS_ESCAPE" _zsuggestion-native-escape
    zle -A "$_ZSUGGESTION_PREVIOUS_ENTER" _zsuggestion-native-enter
    zle -A "$_ZSUGGESTION_PREVIOUS_HISTORY_UP" _zsuggestion-native-history-up
    zle -A "$_ZSUGGESTION_PREVIOUS_HISTORY_UP_APPLICATION" _zsuggestion-native-history-up-application
    zle -A "$_ZSUGGESTION_PREVIOUS_HISTORY_DOWN" _zsuggestion-native-history-down
    zle -A "$_ZSUGGESTION_PREVIOUS_HISTORY_DOWN_APPLICATION" _zsuggestion-native-history-down-application
    zle -A self-insert _zsuggestion-native-self-insert
    typeset -g _ZSUGGESTION_PREVIOUS_SPACE="${$(bindkey ' ')##* }"
    [[ -z "$_ZSUGGESTION_PREVIOUS_SPACE" || "$_ZSUGGESTION_PREVIOUS_SPACE" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_SPACE=self-insert
    zle -A "$_ZSUGGESTION_PREVIOUS_SPACE" _zsuggestion-native-space
    zle -A backward-delete-char _zsuggestion-native-backward-delete
    zle -A bracketed-paste _zsuggestion-native-bracketed-paste
    typeset -g _ZSUGGESTION_PREVIOUS_INTERRUPT="${$(bindkey '^C')##* }"
    [[ -z "$_ZSUGGESTION_PREVIOUS_INTERRUPT" || "$_ZSUGGESTION_PREVIOUS_INTERRUPT" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_INTERRUPT=send-break
    zle -A "$_ZSUGGESTION_PREVIOUS_INTERRUPT" _zsuggestion-native-interrupt
    typeset -g _ZSUGGESTION_PREVIOUS_TAB="${$(bindkey '^I')##* }"
    [[ -z "$_ZSUGGESTION_PREVIOUS_TAB" || "$_ZSUGGESTION_PREVIOUS_TAB" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_TAB=expand-or-complete
    zle -A "$_ZSUGGESTION_PREVIOUS_TAB" _zsuggestion-native-tab
  fi
  if [[ -z "${widgets[_zsuggestion-native-space]:-}" ]]; then
    if [[ -z "${_ZSUGGESTION_PREVIOUS_SPACE:-}" || "$_ZSUGGESTION_PREVIOUS_SPACE" == zsuggestion-space ]]; then
      typeset -g _ZSUGGESTION_PREVIOUS_SPACE="${$(bindkey ' ')##* }"
      if [[ -z "$_ZSUGGESTION_PREVIOUS_SPACE" || "$_ZSUGGESTION_PREVIOUS_SPACE" == "undefined-key" ||
            "$_ZSUGGESTION_PREVIOUS_SPACE" == zsuggestion-space ]]; then
        if [[ -n "${widgets[magic-space]:-}" ]]; then
          _ZSUGGESTION_PREVIOUS_SPACE=magic-space
        else
          _ZSUGGESTION_PREVIOUS_SPACE=self-insert
        fi
      fi
    fi
    zle -A "$_ZSUGGESTION_PREVIOUS_SPACE" _zsuggestion-native-space
  fi
  if [[ -z "${widgets[_zsuggestion-native-escape]:-}" ]]; then
    if [[ -z "${_ZSUGGESTION_PREVIOUS_ESCAPE:-}" || "$_ZSUGGESTION_PREVIOUS_ESCAPE" == zsuggestion-escape ]]; then
      typeset -g _ZSUGGESTION_PREVIOUS_ESCAPE="${$(bindkey '^[')##* }"
      [[ -z "$_ZSUGGESTION_PREVIOUS_ESCAPE" || "$_ZSUGGESTION_PREVIOUS_ESCAPE" == zsuggestion-escape ]] && \
        _ZSUGGESTION_PREVIOUS_ESCAPE=undefined-key
    fi
    zle -A "$_ZSUGGESTION_PREVIOUS_ESCAPE" _zsuggestion-native-escape
  fi
  if [[ -z "${widgets[_zsuggestion-native-interrupt]:-}" ]]; then
    if [[ -z "${_ZSUGGESTION_PREVIOUS_INTERRUPT:-}" || "$_ZSUGGESTION_PREVIOUS_INTERRUPT" == zsuggestion-interrupt ]]; then
      typeset -g _ZSUGGESTION_PREVIOUS_INTERRUPT="${$(bindkey '^C')##* }"
      [[ -z "$_ZSUGGESTION_PREVIOUS_INTERRUPT" || "$_ZSUGGESTION_PREVIOUS_INTERRUPT" == "undefined-key" ||
            "$_ZSUGGESTION_PREVIOUS_INTERRUPT" == zsuggestion-interrupt ]] && \
        _ZSUGGESTION_PREVIOUS_INTERRUPT=send-break
    fi
    zle -A "$_ZSUGGESTION_PREVIOUS_INTERRUPT" _zsuggestion-native-interrupt
  fi
  if [[ -z "${widgets[_zsuggestion-native-enter]:-}" ]]; then
    typeset -g _ZSUGGESTION_PREVIOUS_ENTER="${$(bindkey '^M')##* }"
    [[ -z "$_ZSUGGESTION_PREVIOUS_ENTER" || "$_ZSUGGESTION_PREVIOUS_ENTER" == "undefined-key" ||
          "$_ZSUGGESTION_PREVIOUS_ENTER" == zsuggestion-fuzzy-execute ]] && \
      _ZSUGGESTION_PREVIOUS_ENTER=accept-line
    zle -A "$_ZSUGGESTION_PREVIOUS_ENTER" _zsuggestion-native-enter
  fi
  zle -N zsuggestion-tab _zsuggestion_tab
  bindkey '^I' zsuggestion-tab
  zle -N zsuggestion-complete _zsuggestion_complete
  zle -N zsuggestion-fuzzy-execute _zsuggestion_fuzzy_execute
  zle -N zsuggestion-menu-down _zsuggestion_menu_down
  zle -N zsuggestion-menu-up _zsuggestion_menu_up
  zle -N zsuggestion-shift-tab _zsuggestion_shift_tab
  zle -N zsuggestion-escape _zsuggestion_escape
  zle -N zsuggestion-history-up _zsuggestion_history_up
  zle -N zsuggestion-history-up-application _zsuggestion_history_up_application
  zle -N zsuggestion-history-down _zsuggestion_history_down
  zle -N zsuggestion-history-down-application _zsuggestion_history_down_application
  zle -N zsuggestion-menu-ready _zsuggestion_menu_request_ready
  zle -N zsuggestion-menu-apply _zsuggestion_menu_apply_result
  zle -N zsuggestion-menu-tick _zsuggestion_menu_tick
  zle -C zsuggestion-native-capture .complete-word _zsuggestion_native_capture_widget
  zle -N self-insert _zsuggestion_self_insert
  zle -N zsuggestion-space _zsuggestion_space
  zle -N backward-delete-char _zsuggestion_backward_delete
  zle -N bracketed-paste _zsuggestion_bracketed_paste
  zle -N zsuggestion-interrupt _zsuggestion_interrupt
  bindkey '__ZSUGGESTION_COMPLETION_KEY__' zsuggestion-complete
  bindkey '^N' zsuggestion-menu-down
  bindkey '^K' zsuggestion-menu-up
  bindkey '^[[Z' zsuggestion-shift-tab
  bindkey '^[' zsuggestion-escape
  bindkey '^[[A' zsuggestion-history-up
  bindkey '^[OA' zsuggestion-history-up-application
  bindkey '^[[B' zsuggestion-history-down
  bindkey '^[OB' zsuggestion-history-down-application
  bindkey '^C' zsuggestion-interrupt
  bindkey ' ' zsuggestion-space
  bindkey -N zsuggestion-fuzzy
  bindkey -M zsuggestion-fuzzy -R ' '-'~' self-insert
  bindkey -M zsuggestion-fuzzy ' ' zsuggestion-space
  bindkey -M zsuggestion-fuzzy '^?' backward-delete-char
  bindkey -M zsuggestion-fuzzy '^H' backward-delete-char
  bindkey -M zsuggestion-fuzzy '^I' zsuggestion-tab
  bindkey -M zsuggestion-fuzzy '^N' zsuggestion-menu-down
  bindkey -M zsuggestion-fuzzy '^K' zsuggestion-menu-up
  bindkey -M zsuggestion-fuzzy '^[[A' zsuggestion-menu-up
  bindkey -M zsuggestion-fuzzy '^[OA' zsuggestion-menu-up
  bindkey -M zsuggestion-fuzzy '^[[B' zsuggestion-menu-down
  bindkey -M zsuggestion-fuzzy '^[OB' zsuggestion-menu-down
  bindkey -M zsuggestion-fuzzy '__ZSUGGESTION_COMPLETION_KEY__' zsuggestion-complete
  bindkey -M zsuggestion-fuzzy '^M' zsuggestion-fuzzy-execute
  bindkey -M zsuggestion-fuzzy '^C' zsuggestion-interrupt
  bindkey -M zsuggestion-fuzzy '^[' zsuggestion-escape

  if [[ -n "${HISTFILE:-}" && -r "$HISTFILE" ]]; then
    command zsuggestion import-history --file "${HISTFILE:A}" >/dev/null 2>&1 &!
  fi
fi
"#
    .replace("__ZSUGGESTION_COMPLETION_KEY__", &completion_key)
    .replace("__ZSUGGESTION_COMPLETION_KEY_LABEL__", &completion_key_label)
    .replace("__ZSUGGESTION_UI_MENU_WIDTH__", &menu_width)
    .replace("__ZSUGGESTION_UI_MAX_VISIBLE__", &max_visible)
    .replace("__ZSUGGESTION_COMPLETION_MAX_CANDIDATES__", &max_candidates)
    .replace(
        "__ZSUGGESTION_UI_PROMPT_OFFSET__",
        &settings.ui.prompt_offset.to_string(),
    )
    .replace("__ZSUGGESTION_UI_ACCENT__", &settings.ui.accent)
    .replace("__ZSUGGESTION_UI_TEXT__", &settings.ui.text)
    .replace("__ZSUGGESTION_UI_MUTED__", &settings.ui.muted)
    .replace("__ZSUGGESTION_UI_GHOST__", &settings.ui.ghost)
    .replace(
        "__ZSUGGESTION_UI_SELECTED_BACKGROUND__",
        &settings.ui.selected_background,
    )
    .replace("__ZSUGGESTION_UI_SELECTED_TEXT__", &settings.ui.selected_text)
    .replace(
        "__ZSUGGESTION_UI_SELECTED_SOURCE__",
        &settings.ui.selected_source,
    ))
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
            assert!(write_file_preview(path, directory.path().to_owned()).is_err());
        }
        assert!(!is_env_file(&directory.path().join("example.env")));
    }

    #[test]
    fn previews_ls_without_a_shell() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("alpha.txt"), "alpha").unwrap();
        fs::write(directory.path().join("beta.txt"), "beta").unwrap();

        let lines = command_preview_lines("ls", directory.path().to_owned()).unwrap();
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
        assert!(command_preview_lines("ls; rm -rf /", directory.path().to_owned()).is_err());
        assert!(command_preview_lines("echo hello", directory.path().to_owned()).is_err());
        assert!(command_preview_lines("/bin/ls", directory.path().to_owned()).is_err());
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

    #[test]
    fn zsh_integration_uses_configured_completion_key() {
        let mut settings = Settings::default();
        let integration = zsh_integration(&settings).unwrap();
        assert!(integration.contains("bindkey '^@' zsuggestion-complete"));
        assert!(integration.contains("Ctrl-Space full"));
        assert!(integration.contains("bindkey '^[[Z' zsuggestion-shift-tab"));
        assert!(integration.contains("--format zsh-v3"));
        assert!(integration.contains("_ZSUGGESTION_MENU_REFRESH_TICKS=5"));
        assert!(integration.contains("zle -C zsuggestion-native-capture"));
        assert!(integration.contains("descriptions+=(\"Zsh completion\")"));
        assert!(integration.contains("${(@)region_highlight:#*memo=zsuggestion*}"));
        assert!(integration.contains("_ZSUGGESTION_FOREIGN_HIGHLIGHTS"));
        assert!(integration.contains("_ZSUGGESTION_MENU_RESTORE_INDEX"));
        assert!(!integration.contains("_ZSUGGESTION_MENU_RESTORE_DISPLAY"));
        assert!(!integration.contains("select-pane"));
        assert!(!integration.contains("ASTER_TMUX_SHELL_TITLE"));
        assert!(!integration.contains("zsuggestion-menu-enter"));
        assert!(!integration.contains("bindkey '^M' zsuggestion"));
        assert!(
            integration.contains("bindkey -M zsuggestion-fuzzy '^M' zsuggestion-fuzzy-execute")
        );
        assert!(!integration.contains("__ZSUGGESTION_COMPLETION_KEY__"));
        assert!(!integration.contains("__ZSUGGESTION_UI_"));

        settings.completion.key = "ctrl-x".to_owned();
        settings.ui.selected_background = "#3a3228".to_owned();
        let integration = zsh_integration(&settings).unwrap();
        assert!(integration.contains("bindkey '^X' zsuggestion-complete"));
        assert!(integration.contains("bg=#3a3228"));
    }
}
