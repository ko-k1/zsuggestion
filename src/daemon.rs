use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use fs2::FileExt;

use crate::VERSION;
use crate::commands::CommandCatalog;
use crate::config::{Paths, Settings};
use crate::engine;
use crate::protocol::{PROTOCOL_VERSION, Request, RequestEnvelope, Response};
use crate::store::Store;

const MAX_REQUEST_BYTES: u64 = 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_CONNECTIONS: usize = 64;
const MAX_PATH_BYTES: usize = 16 * 1024;
const MAX_SESSION_ID_BYTES: usize = 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(3);

pub fn serve(paths: Paths, settings: Settings) -> Result<()> {
    paths.ensure_directories()?;
    let _daemon_lock = acquire_daemon_lock(&paths.daemon_lock_file)?;
    prepare_socket(&paths.socket_file)?;
    let listener = UnixListener::bind(&paths.socket_file)
        .with_context(|| format!("failed to bind socket {}", paths.socket_file.display()))?;
    listener.set_nonblocking(true)?;
    fs::set_permissions(&paths.socket_file, fs::Permissions::from_mode(0o600))?;
    let _socket_guard = SocketGuard::new(paths.socket_file.clone())?;

    let store = Arc::new(Mutex::new(Store::open(&paths.database_file)?));
    let database_file = Arc::new(paths.database_file.clone());
    let write_lock = Arc::new(Mutex::new(()));
    let settings = Arc::new(settings);
    let commands = Arc::new(CommandCatalog::discover(
        paths.command_description_cache.clone(),
    ));
    let shutdown = Arc::new(AtomicBool::new(false));
    let mut workers = Vec::new();

    while !shutdown.load(Ordering::Acquire) {
        reap_finished_workers(&mut workers);
        match listener.accept() {
            Ok((mut stream, _)) if workers.len() >= MAX_CONNECTIONS => {
                let _ = write_response(
                    &mut stream,
                    &Response::Error {
                        message: "daemon is at its connection limit".to_owned(),
                    },
                );
            }
            Ok((stream, _)) => {
                let store = Arc::clone(&store);
                let database_file = Arc::clone(&database_file);
                let write_lock = Arc::clone(&write_lock);
                let settings = Arc::clone(&settings);
                let commands = Arc::clone(&commands);
                let shutdown = Arc::clone(&shutdown);
                let worker = thread::Builder::new()
                    .name("zsuggestion-client".to_owned())
                    .spawn(move || {
                        if let Err(error) = handle_connection(
                            stream,
                            &store,
                            &database_file,
                            &write_lock,
                            &settings,
                            &commands,
                            &shutdown,
                        ) {
                            eprintln!("zsuggestion: request failed: {error:#}");
                        }
                    })?;
                workers.push(worker);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => {
                eprintln!("zsuggestion: failed to accept connection: {error}");
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
    for worker in workers {
        let _ = worker.join();
    }
    Ok(())
}

fn reap_finished_workers(workers: &mut Vec<thread::JoinHandle<()>>) {
    let mut index = 0;
    while index < workers.len() {
        if workers[index].is_finished() {
            let worker = workers.swap_remove(index);
            let _ = worker.join();
        } else {
            index += 1;
        }
    }
}

fn prepare_socket(path: &Path) -> Result<()> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if !metadata.file_type().is_socket() {
        bail!("refusing to replace non-socket path {}", path.display());
    }
    if UnixStream::connect(path).is_ok() {
        bail!(
            "zsuggestion daemon is already running at {}",
            path.display()
        );
    }
    fs::remove_file(path)
        .with_context(|| format!("failed to remove stale socket {}", path.display()))
}

fn acquire_daemon_lock(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
        .with_context(|| format!("failed to open daemon lock {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    file.try_lock_exclusive().with_context(|| {
        format!(
            "zsuggestion daemon is already running for {}",
            path.display()
        )
    })?;
    Ok(file)
}

fn handle_connection(
    mut stream: UnixStream,
    store: &Arc<Mutex<Store>>,
    database_file: &Path,
    write_lock: &Mutex<()>,
    settings: &Settings,
    commands: &CommandCatalog,
    shutdown: &AtomicBool,
) -> Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;

    let mut payload = String::new();
    BufReader::new(stream.try_clone()?)
        .take(MAX_REQUEST_BYTES + 1)
        .read_line(&mut payload)?;
    if payload.len() as u64 > MAX_REQUEST_BYTES {
        return write_response(
            &mut stream,
            &Response::Error {
                message: "request exceeds 1 MiB".to_owned(),
            },
        );
    }

    let response = match serde_json::from_str::<RequestEnvelope>(&payload) {
        Ok(envelope) if envelope.version == PROTOCOL_VERSION => dispatch(
            envelope.request,
            store,
            database_file,
            write_lock,
            settings,
            commands,
        ),
        Ok(envelope) => Response::Error {
            message: format!(
                "unsupported protocol version {}; expected {PROTOCOL_VERSION}",
                envelope.version
            ),
        },
        Err(error) => Response::Error {
            message: format!("invalid request: {error}"),
        },
    };
    let should_shutdown = matches!(response, Response::ShuttingDown);
    if should_shutdown {
        shutdown.store(true, Ordering::Release);
    }
    write_response(&mut stream, &response)
}

fn dispatch(
    request: Request,
    store: &Arc<Mutex<Store>>,
    database_file: &Path,
    write_lock: &Mutex<()>,
    settings: &Settings,
    commands: &CommandCatalog,
) -> Response {
    let result: Result<Response> = (|| match request {
        Request::Ping => Ok(Response::Pong {
            version: VERSION.to_owned(),
        }),
        Request::Shutdown => Ok(Response::ShuttingDown),
        Request::Record {
            command,
            cwd,
            exit_code,
            observed_at_ms,
            session_id,
        } => {
            if cwd.len() > MAX_PATH_BYTES {
                bail!("working directory is too long");
            }
            if session_id.len() > MAX_SESSION_ID_BYTES {
                bail!("session ID is too long");
            }
            if observed_at_ms.abs_diff(now_ms()) > 5 * 60 * 1_000 {
                bail!("command timestamp is outside the allowed five-minute window");
            }
            let _write_guard = write_lock.lock().expect("write lock poisoned");
            store.lock().expect("store lock poisoned").record(
                &command,
                &cwd,
                exit_code,
                observed_at_ms,
                &session_id,
                settings.history.ignore_leading_space,
            )?;
            Ok(Response::Recorded)
        }
        Request::Complete {
            buffer,
            cursor_byte,
            cwd,
            limit,
        } => {
            if cwd.len() > MAX_PATH_BYTES {
                bail!("working directory is too long");
            }
            let mut completion = {
                let store = store.lock().expect("store lock poisoned");
                engine::complete(
                    &store,
                    commands,
                    &buffer,
                    cursor_byte,
                    &cwd,
                    limit,
                    settings,
                )?
            };
            let limit = limit
                .unwrap_or(settings.completion.max_candidates)
                .min(settings.completion.max_candidates);
            let paths = engine::filesystem_candidates(&buffer, cursor_byte, &cwd)?;
            engine::merge_filesystem_candidates(&mut completion, paths, limit);
            Ok(Response::Completion(completion))
        }
        Request::Fuzzy { query, cwd, limit } => {
            if cwd.len() > MAX_PATH_BYTES {
                bail!("working directory is too long");
            }
            let completion = engine::fuzzy(
                &store.lock().expect("store lock poisoned"),
                commands,
                &query,
                &cwd,
                limit,
                settings,
            )?;
            Ok(Response::Completion(completion))
        }
        Request::ImportHistory { path } => {
            if path.len() > MAX_PATH_BYTES {
                bail!("history path is too long");
            }
            let _write_guard = write_lock.lock().expect("write lock poisoned");
            let mut import_store = Store::open(database_file)?;
            let result = import_store
                .import_zsh_history(Path::new(&path), settings.history.ignore_leading_space)?;
            Ok(Response::Imported {
                imported: result.imported,
                skipped: result.skipped,
            })
        }
    })();

    result.unwrap_or_else(|error| Response::Error {
        message: format!("{error:#}"),
    })
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn write_response(stream: &mut UnixStream, response: &Response) -> Result<()> {
    let mut payload = serde_json::to_vec(response)?;
    if payload.len() > MAX_RESPONSE_BYTES {
        payload = serde_json::to_vec(&Response::Error {
            message: "response exceeds 1 MiB".to_owned(),
        })?;
    }
    stream.write_all(&payload)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

struct SocketGuard {
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl SocketGuard {
    fn new(path: PathBuf) -> Result<Self> {
        let metadata = fs::symlink_metadata(&path)?;
        Ok(Self {
            path,
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let Ok(metadata) = fs::symlink_metadata(&self.path) else {
            return;
        };
        if metadata.file_type().is_socket()
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn refuses_to_remove_a_non_socket_path() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("zsuggestion.sock");
        fs::write(&path, "keep me").unwrap();

        assert!(prepare_socket(&path).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "keep me");
    }

    #[test]
    fn daemon_lock_is_exclusive() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("daemon.lock");
        let first = acquire_daemon_lock(&path).unwrap();
        assert!(acquire_daemon_lock(&path).is_err());
        drop(first);
        let mut reacquired = None;
        for _ in 0..100 {
            match acquire_daemon_lock(&path) {
                Ok(lock) => {
                    reacquired = Some(lock);
                    break;
                }
                Err(_) => thread::sleep(Duration::from_millis(10)),
            }
        }
        assert!(
            reacquired.is_some(),
            "daemon lock was not released after the holder dropped it"
        );
    }
}
