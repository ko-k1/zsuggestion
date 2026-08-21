use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use fs2::FileExt;

use crate::config::Paths;
use crate::protocol::{PROTOCOL_VERSION, Request, RequestEnvelope, Response};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const IO_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;

pub fn request(paths: &Paths, request: Request) -> Result<Response> {
    match request_once(paths, Request::Ping) {
        Ok(Response::Pong { .. }) => {}
        Ok(Response::Error { message }) if message.contains("unsupported protocol version") => {
            replace_previous_daemon(paths)?;
        }
        _ => {
            start_daemon(paths)?;
            wait_for_daemon(paths)?;
        }
    }
    request_once(paths, request)
}

pub fn request_idempotent(paths: &Paths, request: Request) -> Result<Response> {
    match request_once(paths, request.clone()) {
        Ok(Response::Error { message }) if message.contains("unsupported protocol version") => {
            replace_previous_daemon(paths)?;
            request_once(paths, request)
        }
        Ok(response) => Ok(response),
        Err(_) => {
            if request_once(paths, Request::Ping).is_err() {
                start_daemon(paths)?;
                wait_for_daemon(paths)?;
            }
            request_once(paths, request)
        }
    }
}

pub fn request_once(paths: &Paths, request: Request) -> Result<Response> {
    request_once_version(paths, request, PROTOCOL_VERSION)
}

fn request_once_version(paths: &Paths, request: Request, version: u32) -> Result<Response> {
    let mut stream = UnixStream::connect(&paths.socket_file)
        .with_context(|| format!("failed to connect to {}", paths.socket_file.display()))?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;

    let envelope = RequestEnvelope { version, request };
    serde_json::to_writer(&mut stream, &envelope)?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    let mut line = String::new();
    BufReader::new(stream)
        .take(MAX_RESPONSE_BYTES + 1)
        .read_line(&mut line)?;
    if line.len() as u64 > MAX_RESPONSE_BYTES {
        bail!("daemon response exceeds 1 MiB");
    }
    if line.is_empty() {
        bail!("daemon closed the connection without a response");
    }
    serde_json::from_str(&line).context("daemon returned an invalid response")
}

fn replace_previous_daemon(paths: &Paths) -> Result<()> {
    for version in (0..PROTOCOL_VERSION).rev() {
        if matches!(
            request_once_version(paths, Request::Shutdown, version),
            Ok(Response::ShuttingDown)
        ) {
            break;
        }
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let socket_closed = UnixStream::connect(&paths.socket_file).is_err();
        if socket_closed && daemon_lock_is_available(paths) {
            start_daemon(paths)?;
            return wait_for_daemon(paths);
        }
        thread::sleep(Duration::from_millis(20));
    }
    bail!("previous zsuggestion daemon did not release its socket and lock")
}

fn daemon_lock_is_available(paths: &Paths) -> bool {
    let Ok(file) = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&paths.daemon_lock_file)
    else {
        return false;
    };
    if file.try_lock_exclusive().is_err() {
        return false;
    }
    let _ = FileExt::unlock(&file);
    true
}

fn start_daemon(paths: &Paths) -> Result<()> {
    paths.ensure_directories()?;
    let executable = std::env::current_exe().context("failed to locate zsuggestion executable")?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.state_dir.join("daemon.log"))?;
    let error_log = log.try_clone()?;

    let mut command = Command::new(executable);
    command
        .arg("daemon")
        .current_dir(&paths.state_dir)
        .env("ZSUGGESTION_CONFIG", &paths.config_file)
        .env("ZSUGGESTION_STATE_DIR", &paths.state_dir)
        .env("ZSUGGESTION_SOCKET", &paths.socket_file)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(error_log));

    // A new session keeps the shared daemon alive when the shell, tmux client,
    // or SSH connection that first requested it exits.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command.spawn().context("failed to start zsuggestion daemon")?;
    Ok(())
}

fn wait_for_daemon(paths: &Paths) -> Result<()> {
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    while Instant::now() < deadline {
        if let Ok(Response::Pong { .. }) = request_once(paths, Request::Ping) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(20));
    }
    bail!(
        "zsuggestion daemon did not become ready at {}",
        paths.socket_file.display()
    )
}
