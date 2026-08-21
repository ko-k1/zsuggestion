use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};

const MAX_COMMAND_BYTES: usize = 64 * 1024;
const MAX_HISTORY_BYTES: u64 = 32 * 1024 * 1024;
const MAX_HISTORY_ENTRIES: usize = 250_000;
const SCHEMA_VERSION: i64 = 1;

#[derive(Debug)]
pub struct Store {
    connection: Connection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportResult {
    pub imported: usize,
    pub skipped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryCandidate {
    pub command: String,
    pub same_cwd: bool,
    pub uses: usize,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let connection = Connection::open(path)
            .with_context(|| format!("failed to open database {}", path.display()))?;
        Self::from_connection(connection)
    }

    pub fn in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> Result<Self> {
        connection.busy_timeout(std::time::Duration::from_secs(2))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "case_sensitive_like", "ON")?;
        let schema_version: i64 =
            connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if schema_version > SCHEMA_VERSION {
            bail!(
                "database schema {schema_version} is newer than supported schema {SCHEMA_VERSION}"
            );
        }
        if schema_version == 0 {
            connection.execute_batch(
                "
            CREATE TABLE IF NOT EXISTS command_events (
                id              INTEGER PRIMARY KEY,
                command         TEXT NOT NULL,
                cwd             TEXT NOT NULL,
                observed_at_ms  INTEGER NOT NULL,
                exit_code       INTEGER,
                source          TEXT NOT NULL,
                source_key      TEXT,
                UNIQUE(source, source_key)
            );
            CREATE INDEX IF NOT EXISTS command_events_command_idx
                ON command_events(command);
            CREATE INDEX IF NOT EXISTS command_events_cwd_idx
                ON command_events(cwd);

            CREATE TABLE IF NOT EXISTS history_imports (
                path            TEXT PRIMARY KEY,
                size_bytes      INTEGER NOT NULL,
                modified_ns     INTEGER NOT NULL
            );
            PRAGMA user_version = 1;
            ",
            )?;
        }
        Ok(Self { connection })
    }

    pub fn record(
        &self,
        command: &str,
        cwd: &str,
        exit_code: i32,
        observed_at_ms: i64,
        session_id: &str,
        ignore_leading_space: bool,
    ) -> Result<bool> {
        if !eligible_command(command, ignore_leading_space) {
            return Ok(false);
        }
        self.connection.execute(
            "INSERT INTO command_events
                (command, cwd, observed_at_ms, exit_code, source, source_key)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
            params![
                command,
                cwd,
                observed_at_ms,
                exit_code,
                format!("session:{session_id}")
            ],
        )?;
        Ok(true)
    }

    pub fn history_candidates(
        &self,
        prefix: &str,
        cwd: &str,
        limit: usize,
        successful_first: bool,
    ) -> Result<Vec<HistoryCandidate>> {
        let pattern = format!("{}%", escape_like(prefix));
        let success_order = i64::from(successful_first);
        let mut statement = self.connection.prepare(
            "SELECT
                command,
                MAX(CASE WHEN cwd = ?2 THEN 1 ELSE 0 END) AS same_cwd,
                SUM(CASE WHEN exit_code = 0 THEN 1 ELSE 0 END) AS successes,
                MAX(observed_at_ms) AS latest,
                COUNT(*) AS uses
             FROM command_events
             WHERE command LIKE ?1 ESCAPE '\\' AND command <> ?3
              GROUP BY command
              ORDER BY
                latest DESC,
                same_cwd DESC,
                CASE WHEN ?5 = 1 AND successes > 0 THEN 1 ELSE 0 END DESC,
                uses DESC,
                command ASC
             LIMIT ?4",
        )?;
        let rows = statement.query_map(
            params![pattern, cwd, prefix, limit as i64, success_order],
            |row| {
                Ok(HistoryCandidate {
                    command: row.get(0)?,
                    same_cwd: row.get::<_, i64>(1)? != 0,
                    uses: row.get::<_, i64>(4)?.max(0) as usize,
                })
            },
        )?;
        rows.collect::<rusqlite::Result<Vec<HistoryCandidate>>>()
            .context("failed to read history candidates")
    }

    pub fn history_inventory(
        &self,
        cwd: &str,
        limit: usize,
        successful_first: bool,
    ) -> Result<Vec<HistoryCandidate>> {
        let success_order = i64::from(successful_first);
        let mut statement = self.connection.prepare(
            "SELECT
                command,
                MAX(CASE WHEN cwd = ?1 THEN 1 ELSE 0 END) AS same_cwd,
                SUM(CASE WHEN exit_code = 0 THEN 1 ELSE 0 END) AS successes,
                MAX(observed_at_ms) AS latest,
                COUNT(*) AS uses
             FROM command_events
              GROUP BY command
              ORDER BY
                latest DESC,
                same_cwd DESC,
                CASE WHEN ?3 = 1 AND successes > 0 THEN 1 ELSE 0 END DESC,
                uses DESC,
                command ASC
             LIMIT ?2",
        )?;
        let rows = statement.query_map(params![cwd, limit as i64, success_order], |row| {
            Ok(HistoryCandidate {
                command: row.get(0)?,
                same_cwd: row.get::<_, i64>(1)? != 0,
                uses: row.get::<_, i64>(4)?.max(0) as usize,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<HistoryCandidate>>>()
            .context("failed to read fuzzy history inventory")
    }

    pub fn import_zsh_history(
        &mut self,
        path: &Path,
        ignore_leading_space: bool,
    ) -> Result<ImportResult> {
        let canonical = path
            .canonicalize()
            .with_context(|| format!("failed to resolve history file {}", path.display()))?;
        let metadata = fs::metadata(&canonical)
            .with_context(|| format!("failed to inspect history file {}", canonical.display()))?;
        if !metadata.is_file() {
            bail!(
                "history path is not a regular file: {}",
                canonical.display()
            );
        }
        if metadata.len() > MAX_HISTORY_BYTES {
            bail!(
                "history file exceeds {} MiB: {}",
                MAX_HISTORY_BYTES / (1024 * 1024),
                canonical.display()
            );
        }
        let modified_ns = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .min(i64::MAX as u128) as i64;
        let size_bytes = metadata.len().min(i64::MAX as u64) as i64;
        let source = format!("zsh:{}", canonical.display());

        let previous: Option<(i64, i64)> = self
            .connection
            .query_row(
                "SELECT size_bytes, modified_ns FROM history_imports WHERE path = ?1",
                params![canonical.to_string_lossy()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if previous == Some((size_bytes, modified_ns)) {
            return Ok(ImportResult {
                imported: 0,
                skipped: true,
            });
        }

        let mut content = Vec::new();
        fs::File::open(&canonical)
            .with_context(|| format!("failed to open history file {}", canonical.display()))?
            .take(MAX_HISTORY_BYTES + 1)
            .read_to_end(&mut content)
            .with_context(|| format!("failed to read history file {}", canonical.display()))?;
        if content.len() as u64 > MAX_HISTORY_BYTES {
            bail!("history file grew beyond the import limit while reading");
        }
        let content = String::from_utf8(content).context("history file is not valid UTF-8")?;
        let entries = parse_zsh_history(&content, ignore_leading_space)?;

        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM command_events WHERE source = ?1",
            params![source],
        )?;
        {
            let mut insert = transaction.prepare(
                "INSERT INTO command_events
                    (command, cwd, observed_at_ms, exit_code, source, source_key)
                 VALUES (?1, '', ?2, NULL, ?3, ?4)",
            )?;
            for (index, entry) in entries.iter().enumerate() {
                insert.execute(params![
                    entry.command,
                    entry.observed_at_ms,
                    source,
                    index.to_string()
                ])?;
            }
        }
        transaction.execute(
            "INSERT INTO history_imports (path, size_bytes, modified_ns)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(path) DO UPDATE SET
                size_bytes = excluded.size_bytes,
                modified_ns = excluded.modified_ns",
            params![canonical.to_string_lossy(), size_bytes, modified_ns],
        )?;
        transaction.commit()?;

        Ok(ImportResult {
            imported: entries.len(),
            skipped: false,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
struct HistoryEntry {
    command: String,
    observed_at_ms: i64,
}

fn parse_zsh_history(content: &str, ignore_leading_space: bool) -> Result<Vec<HistoryEntry>> {
    let mut entries = Vec::new();
    let mut lines = content.lines().enumerate();
    while let Some((index, line)) = lines.next() {
        if line.ends_with('\\') {
            let mut continued = true;
            while continued {
                let Some((_, continuation)) = lines.next() else {
                    break;
                };
                continued = continuation.ends_with('\\');
            }
            continue;
        }

        let (command, observed_at_ms) = parse_zsh_history_line(line, index as i64);
        if eligible_command(command, ignore_leading_space) {
            entries.push(HistoryEntry {
                command: command.to_owned(),
                observed_at_ms,
            });
            if entries.len() > MAX_HISTORY_ENTRIES {
                bail!("history file exceeds {MAX_HISTORY_ENTRIES} entries");
            }
        }
    }
    Ok(entries)
}

fn parse_zsh_history_line(line: &str, fallback_order: i64) -> (&str, i64) {
    let Some(metadata_and_command) = line.strip_prefix(": ") else {
        return (line, fallback_order);
    };
    let Some((metadata, command)) = metadata_and_command.split_once(';') else {
        return (line, fallback_order);
    };
    let timestamp = metadata
        .split(':')
        .next()
        .and_then(|value| value.parse::<i64>().ok())
        .map(|seconds| seconds.saturating_mul(1_000))
        .unwrap_or(fallback_order);
    (command, timestamp)
}

fn eligible_command(command: &str, ignore_leading_space: bool) -> bool {
    if command.is_empty() || command.len() > MAX_COMMAND_BYTES {
        return false;
    }
    if ignore_leading_space && command.starts_with(char::is_whitespace) {
        return false;
    }
    !command.chars().any(|character| {
        character == '\0' || (character.is_control() && !matches!(character, '\t'))
    })
}

fn escape_like(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn ranks_recent_history_before_directory_success_and_usage() {
        let store = Store::in_memory().unwrap();
        for observed_at_ms in 10..20 {
            store
                .record("git switch main", "/repo", 0, observed_at_ms, "one", true)
                .unwrap();
        }
        store
            .record("git status", "/other", 0, 300, "one", true)
            .unwrap();
        store
            .record("git stash", "/repo", 1, 200, "one", true)
            .unwrap();

        let candidates = store
            .history_candidates("git s", "/repo", 10, true)
            .unwrap();
        assert!(!candidates[0].same_cwd);
        assert!(candidates[1].same_cwd);
        assert_eq!(
            candidates
                .into_iter()
                .map(|candidate| candidate.command)
                .collect::<Vec<_>>(),
            vec!["git status", "git stash", "git switch main"]
        );
    }

    #[test]
    fn fuzzy_inventory_is_not_prefix_limited() {
        let store = Store::in_memory().unwrap();
        store
            .record("cargo test", "/repo", 0, 100, "one", true)
            .unwrap();
        store
            .record("git status", "/other", 0, 200, "one", true)
            .unwrap();

        let candidates = store.history_inventory("/repo", 10, true).unwrap();
        assert_eq!(candidates[0].command, "git status");
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.command == "cargo test")
        );
    }

    #[test]
    fn parses_extended_zsh_history() {
        let entries =
            parse_zsh_history(": 1700000000:4;git status\nplain command\n", true).unwrap();
        assert_eq!(entries[0].command, "git status");
        assert_eq!(entries[0].observed_at_ms, 1_700_000_000_000);
        assert_eq!(entries[1].command, "plain command");
    }

    #[test]
    fn escapes_like_metacharacters() {
        assert_eq!(escape_like("echo 100%_done"), "echo 100\\%\\_done");
    }

    #[test]
    fn ignores_leading_space_when_configured() {
        assert!(!eligible_command(" secret command", true));
        assert!(eligible_command(" secret command", false));
    }

    #[test]
    fn imports_a_history_file_once_until_it_changes() {
        let directory = tempdir().unwrap();
        let history = directory.path().join("history");
        fs::write(
            &history,
            ": 1700000000:0;cargo test\n: 1700000001:0;cargo check\n",
        )
        .unwrap();
        let mut store = Store::in_memory().unwrap();

        let first = store.import_zsh_history(&history, true).unwrap();
        assert_eq!(first.imported, 2);
        assert!(!first.skipped);

        let second = store.import_zsh_history(&history, true).unwrap();
        assert!(second.skipped);
        assert_eq!(
            store
                .history_candidates("cargo ", "/repo", 10, true)
                .unwrap()
                .into_iter()
                .map(|candidate| candidate.command)
                .collect::<Vec<_>>(),
            vec!["cargo check", "cargo test"]
        );
    }

    #[test]
    fn skips_multiline_history_entries_conservatively() {
        let entries = parse_zsh_history(
            ": 1700000000:0;echo first \\\n+continued\n: 1700000001:0;git status\n",
            true,
        )
        .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].command, "git status");
    }

    #[test]
    fn rejects_invalid_utf8_history() {
        let directory = tempdir().unwrap();
        let history = directory.path().join("history");
        fs::write(&history, [0xff, 0xfe]).unwrap();
        let mut store = Store::in_memory().unwrap();
        assert!(store.import_zsh_history(&history, true).is_err());
    }

    #[test]
    fn rejects_newer_database_schema() {
        let connection = Connection::open_in_memory().unwrap();
        connection.pragma_update(None, "user_version", 2).unwrap();
        assert!(Store::from_connection(connection).is_err());
    }
}
