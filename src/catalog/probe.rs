//! Bounded `man` and `--help` probes that feed the help/man parsers.
//!
//! Each probe runs in its own process group with a hard timeout, a file-size
//! rlimit, and output redirected to an owner-private temporary file.

use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use super::parse::{
    parse_help_description, parse_man_description, parse_option_values, parse_options,
    parse_subcommands,
};
use super::{DescriptionJob, Enrichment, TEMP_SEQUENCE};

const OUTPUT_MAX_BYTES: u64 = 64 * 1024;
const MAN_TIMEOUT: Duration = Duration::from_millis(1_500);
const HELP_TIMEOUT: Duration = Duration::from_millis(1_000);

pub(super) fn discover_enrichment(job: &DescriptionJob, output_dir: &Path) -> Enrichment {
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
fn help_enrichment(job: &DescriptionJob, output_dir: &Path) -> Option<Enrichment> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let mut command = Command::new(&job.path);
    command
        .arg("--help")
        .env_clear()
        .env("HOME", home)
        .env("LC_ALL", "C")
        .env("NO_COLOR", "1")
        .env("PAGER", "cat")
        .env("MANPAGER", "cat")
        .env("TERM", "dumb")
        .current_dir("/")
        .stdin(Stdio::null())
        .process_group(0);
    let output = run_bounded(command, output_dir, HELP_TIMEOUT)?;
    Some(Enrichment {
        description: parse_help_description(&job.name, &output),
        options: parse_options(&output),
        subcommands: parse_subcommands(&output),
        option_values: parse_option_values(&output),
    })
}

pub(super) fn run_bounded(
    mut command: Command,
    output_dir: &Path,
    timeout: Duration,
) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn description_process_has_a_hard_timeout() {
        let Some(sleep) = ["sleep", "/bin/sleep"]
            .into_iter()
            .find(|name| Command::new(name).arg("--version").output().is_ok())
            .map(str::to_owned)
        else {
            eprintln!("skipping timeout test because sleep is unavailable");
            return;
        };
        let directory = tempdir().unwrap();
        let mut command = Command::new(sleep);
        command.arg("2");
        let started = Instant::now();

        assert_eq!(
            run_bounded(command, directory.path(), Duration::from_millis(30)).as_deref(),
            Some("")
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
