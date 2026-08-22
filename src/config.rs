use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub completion: CompletionSettings,
    pub history: HistorySettings,
    pub ui: UiSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct CompletionSettings {
    pub max_candidates: usize,
    pub accept: AcceptMode,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct HistorySettings {
    pub ignore_leading_space: bool,
    pub successful_first: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct UiSettings {
    pub menu_width: usize,
    pub max_visible: usize,
    pub prompt_offset: usize,
    pub accent: String,
    pub text: String,
    pub muted: String,
    pub ghost: String,
    pub selected_background: String,
    pub selected_text: String,
    pub selected_source: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AcceptMode {
    Segment,
    Full,
}

impl Default for CompletionSettings {
    fn default() -> Self {
        Self {
            max_candidates: 8,
            accept: AcceptMode::Full,
            key: "ctrl-space".to_owned(),
        }
    }
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self {
            ignore_leading_space: true,
            successful_first: true,
        }
    }
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            menu_width: 64,
            max_visible: 6,
            prompt_offset: 2,
            accent: "14".to_owned(),
            text: "7".to_owned(),
            muted: "8".to_owned(),
            ghost: "8".to_owned(),
            selected_background: "8".to_owned(),
            selected_text: "15".to_owned(),
            selected_source: "0".to_owned(),
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read config {}", path.display()))?;
        let settings: Self = toml::from_str(&content)
            .with_context(|| format!("failed to parse config {}", path.display()))?;
        settings.validate()?;
        Ok(settings)
    }

    pub fn write_default(path: &Path) -> Result<bool> {
        if path.exists() {
            return Ok(false);
        }
        let parent = path
            .parent()
            .context("config path has no parent directory")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
        fs::write(path, DEFAULT_CONFIG)
            .with_context(|| format!("failed to write config {}", path.display()))?;
        set_owner_only_file(path)?;
        Ok(true)
    }

    fn validate(&self) -> Result<()> {
        if !(1..=100).contains(&self.completion.max_candidates) {
            bail!("completion.max_candidates must be between 1 and 100");
        }
        completion_key_sequence(&self.completion.key)?;
        if !(40..=120).contains(&self.ui.menu_width) {
            bail!("ui.menu_width must be between 40 and 120");
        }
        if !(1..=10).contains(&self.ui.max_visible) {
            bail!("ui.max_visible must be between 1 and 10");
        }
        if self.ui.prompt_offset > 40 {
            bail!("ui.prompt_offset must not exceed 40");
        }
        for (name, color) in [
            ("accent", &self.ui.accent),
            ("text", &self.ui.text),
            ("muted", &self.ui.muted),
            ("ghost", &self.ui.ghost),
            ("selected_background", &self.ui.selected_background),
            ("selected_text", &self.ui.selected_text),
            ("selected_source", &self.ui.selected_source),
        ] {
            validate_color(name, color)?;
        }
        Ok(())
    }
}

fn validate_color(name: &str, color: &str) -> Result<()> {
    let ansi = color.parse::<u8>().is_ok();
    let rgb = color.len() == 7
        && color.starts_with('#')
        && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit());
    if !ansi && !rgb {
        bail!("ui.{name} must be an ANSI color from 0 to 255 or #RRGGBB");
    }
    Ok(())
}

pub fn completion_key_sequence(key: &str) -> Result<String> {
    match key {
        "ctrl-space" => Ok("^@".to_owned()),
        _ => {
            let Some(letter) = key.strip_prefix("ctrl-") else {
                bail!("completion.key must be ctrl-space or ctrl-a through ctrl-z");
            };
            if letter.len() != 1 || !letter.as_bytes()[0].is_ascii_lowercase() {
                bail!("completion.key must be ctrl-space or ctrl-a through ctrl-z");
            }
            if matches!(letter, "i" | "j" | "k" | "m" | "n") {
                bail!("completion.key conflicts with a zsuggestion menu control");
            }
            Ok(format!("^{}", letter.to_ascii_uppercase()))
        }
    }
}

pub const DEFAULT_CONFIG: &str = r#"[completion]
# zsuggestion abstains instead of filling this list with low-confidence candidates.
max_candidates = 8

# Accept the highlighted completion with Ctrl-Space. Also accepts "ctrl-a"
# through "ctrl-z" (excluding reserved menu controls).
key = "ctrl-space"

# Ctrl-Space accepts the whole suggestion; Tab always advances one segment.
accept = "full"

[history]
# Match common shell history privacy behavior.
ignore_leading_space = true

# Prefer successful commands when recency is tied.
successful_first = true

[ui]
# ANSI palette indexes adapt to the active terminal theme. #RRGGBB is also valid.
menu_width = 64
max_visible = 6
prompt_offset = 2
accent = "14"
text = "7"
muted = "8"
ghost = "8"
selected_background = "8"
selected_text = "15"
selected_source = "0"
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config_file: PathBuf,
    pub state_dir: PathBuf,
    pub database_file: PathBuf,
    pub command_description_cache: PathBuf,
    pub daemon_lock_file: PathBuf,
    pub socket_file: PathBuf,
}

impl Paths {
    pub fn discover() -> Result<Self> {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .context("HOME is not set")?;

        let config_file = env::var_os("ZSUGGESTION_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join(".config"))
                    .join("zsuggestion/config.toml")
            });
        let config_file = absolute_path(config_file)?;

        let state_dir = env::var_os("ZSUGGESTION_STATE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                env::var_os("XDG_STATE_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join(".local/state"))
                    .join("zsuggestion")
            });
        let state_dir = absolute_path(state_dir)?;

        let socket_file = env::var_os("ZSUGGESTION_SOCKET")
            .map(PathBuf::from)
            .unwrap_or_else(|| state_dir.join("zsuggestion.sock"));
        let socket_file = absolute_path(socket_file)?;

        Ok(Self {
            config_file,
            command_description_cache: state_dir.join("command-descriptions.json"),
            database_file: state_dir.join("history.sqlite3"),
            daemon_lock_file: state_dir.join("daemon.lock"),
            state_dir,
            socket_file,
        })
    }

    pub fn ensure_directories(&self) -> Result<()> {
        create_private_dir(&self.state_dir)?;
        if let Some(parent) = self.socket_file.parent() {
            create_private_dir(parent)?;
        }
        Ok(())
    }
}

fn absolute_path(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path);
    }
    Ok(env::current_dir()
        .context("failed to resolve current directory")?
        .join(path))
}

fn create_private_dir(path: &Path) -> Result<()> {
    let existed = path.exists();
    fs::create_dir_all(path)
        .with_context(|| format!("failed to create directory {}", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        if !existed {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .with_context(|| format!("failed to secure directory {}", path.display()))?;
        }
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
            bail!(
                "directory is not owned by the current user: {}",
                path.display()
            );
        }
        if metadata.mode() & 0o077 != 0 {
            bail!(
                "directory must not be accessible by group or other users: {}",
                path.display()
            );
        }
    }
    Ok(())
}

fn set_owner_only_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to secure file {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use tempfile::tempdir;

    #[test]
    fn default_config_round_trips() {
        let parsed: Settings = toml::from_str(DEFAULT_CONFIG).unwrap();
        assert_eq!(parsed, Settings::default());
    }

    #[test]
    fn rejects_excessive_candidate_limit() {
        let settings: Settings =
            toml::from_str("[completion]\nmax_candidates = 101\naccept = \"segment\"\n").unwrap();
        assert!(settings.validate().is_err());
    }

    #[test]
    fn validates_completion_key() {
        assert_eq!(completion_key_sequence("ctrl-space").unwrap(), "^@");
        assert_eq!(completion_key_sequence("ctrl-x").unwrap(), "^X");
        assert!(completion_key_sequence("tab").is_err());
        assert!(completion_key_sequence("shift-tab").is_err());
        assert!(completion_key_sequence("ctrl-i").is_err());
        assert!(completion_key_sequence("ctrl-k").is_err());
        assert!(completion_key_sequence("alt-space").is_err());
    }

    #[test]
    fn validates_ui_colors() {
        assert!(validate_color("accent", "4").is_ok());
        assert!(validate_color("accent", "255").is_ok());
        assert!(validate_color("accent", "#5f87af").is_ok());
        assert!(validate_color("accent", "256").is_err());
        assert!(validate_color("accent", "blue").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn writing_config_does_not_change_existing_parent_permissions() {
        let directory = tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755)).unwrap();
        let config = directory.path().join("config.toml");

        assert!(Settings::write_default(&config).unwrap());
        assert_eq!(
            fs::metadata(directory.path()).unwrap().mode() & 0o777,
            0o755
        );
        assert_eq!(fs::metadata(config).unwrap().mode() & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_insecure_existing_state_directory_without_chmod() {
        let directory = tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755)).unwrap();

        assert!(create_private_dir(directory.path()).is_err());
        assert_eq!(
            fs::metadata(directory.path()).unwrap().mode() & 0o777,
            0o755
        );
        assert_eq!(fs::metadata(directory.path()).unwrap().uid(), unsafe {
            libc::geteuid()
        });
    }
}
