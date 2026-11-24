//! Command-line interface: argument parsing and subcommand dispatch.
//!
//! `main.rs` is a thin entry point; all clap definitions, request building,
//! and response handling live here.

use std::io::{self, Read, Write};
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand, ValueEnum};

use crate::config::{Paths, Settings};
use crate::output::{self, OutputFormat};
use crate::shell;
use crate::transport::protocol::{Alias, Request, Response};
use crate::transport::{client, daemon};
use crate::{clock, preview};

/// Upper bound for command text and buffers read from arguments or stdin.
const MAX_INPUT_BYTES: u64 = 64 * 1024;

#[derive(Debug, Parser)]
#[command(name = "zsuggestion", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
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
        /// Maximum candidates per daemon tier.
        #[arg(long)]
        limit: Option<usize>,
        /// A shell alias relevant to the buffer; may be repeated.
        #[arg(long = "alias", value_name = "NAME=VALUE", value_parser = parse_alias)]
        aliases: Vec<Alias>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
    },
    /// Fuzzy-rank shared history and installed commands with fzf.
    Fuzzy {
        #[arg(long, default_value = "")]
        query: String,
        #[arg(long)]
        cwd: PathBuf,
        /// Maximum candidates.
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
pub enum Shell {
    Zsh,
}

pub fn run(cli: Cli) -> Result<()> {
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
            let request = Request::Record {
                command: read_input(command, stdin)?,
                cwd: path_string(cwd),
                exit_code,
                observed_at_ms: clock::now_ms(),
                session_id: session,
            };
            match client::request(&paths, request)? {
                Response::Recorded => Ok(()),
                response => Err(unexpected(response)),
            }
        }
        Command::Complete {
            buffer,
            stdin,
            cursor,
            cwd,
            limit,
            aliases,
            format,
        } => {
            let request = Request::Complete {
                buffer: read_input(buffer, stdin)?,
                cursor_byte: cursor,
                cwd: path_string(cwd),
                limit,
                aliases,
            };
            print_completion(client::request_idempotent(&paths, request)?, format)
        }
        Command::Fuzzy {
            query,
            cwd,
            limit,
            format,
        } => {
            let request = Request::Fuzzy {
                query,
                cwd: path_string(cwd),
                limit,
            };
            print_completion(client::request_idempotent(&paths, request)?, format)
        }
        Command::PreviewFile { path, cwd } => {
            preview::write_file_preview(&mut io::stdout().lock(), path, &cwd)
        }
        Command::Preview { line, cwd } => {
            preview::write_command_preview(&mut io::stdout().lock(), &line, &cwd)
        }
        Command::ImportHistory { file } => {
            let file = file
                .canonicalize()
                .with_context(|| format!("failed to resolve history file {}", file.display()))?;
            let request = Request::ImportHistory {
                path: path_string(file),
            };
            match client::request(&paths, request)? {
                Response::Imported { imported, skipped } => {
                    if !skipped {
                        eprintln!("zsuggestion: imported {imported} history entries");
                    }
                    Ok(())
                }
                response => Err(unexpected(response)),
            }
        }
        Command::Init { shell: Shell::Zsh } => {
            Settings::write_default(&paths.config_file)?;
            let settings = Settings::load(&paths.config_file)?;
            print!("{}", shell::zsh_integration(&settings)?);
            Ok(())
        }
        Command::Doctor => doctor(&paths),
        Command::Stop => match client::request_once(&paths, Request::Shutdown)? {
            Response::ShuttingDown => Ok(()),
            response => Err(unexpected(response)),
        },
    }
}

fn doctor(paths: &Paths) -> Result<()> {
    Settings::load(&paths.config_file)?;
    let version = match client::request(paths, Request::Ping)? {
        Response::Pong { version } => version,
        response => return Err(unexpected(response)),
    };
    println!("zsuggestion {version}");
    println!("config: {}", paths.config_file.display());
    println!("database: {}", paths.database_file.display());
    println!("socket: {}", paths.socket_file.display());
    println!("status: ready");
    Ok(())
}

fn print_completion(response: Response, format: OutputFormat) -> Result<()> {
    let Response::Completion(completion) = response else {
        return Err(unexpected(response));
    };
    let mut stdout = io::stdout().lock();
    output::write_completion(&mut stdout, &completion, format)?;
    stdout.flush()?;
    Ok(())
}

/// Converts a daemon error, or any response the caller did not expect, into
/// an error.
fn unexpected(response: Response) -> anyhow::Error {
    match response {
        Response::Error { message } => anyhow!(message),
        response => anyhow!("unexpected daemon response: {response:?}"),
    }
}

fn parse_alias(value: &str) -> Result<Alias, String> {
    match value.split_once('=') {
        Some((name, value)) if !name.is_empty() => Ok(Alias {
            name: name.to_owned(),
            value: value.to_owned(),
        }),
        _ => Err("expected NAME=VALUE".to_owned()),
    }
}

fn path_string(path: PathBuf) -> String {
    path.to_string_lossy().into_owned()
}

fn read_input(value: Option<String>, from_stdin: bool) -> Result<String> {
    if !from_stdin {
        return value.context("missing input value");
    }
    let mut bytes = Vec::new();
    io::stdin()
        .take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_INPUT_BYTES {
        bail!("stdin input exceeds {MAX_INPUT_BYTES} bytes");
    }
    String::from_utf8(bytes).context("stdin input is not valid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_aliases() {
        let alias = parse_alias("g=git status").unwrap();
        assert_eq!(alias.name, "g");
        assert_eq!(alias.value, "git status");
        assert!(parse_alias("=git").is_err());
        assert!(parse_alias("g").is_err());
    }
}
