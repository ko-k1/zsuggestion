//! ZLE (Zsh Line Editor) integration tests.
//!
//! Each test starts an isolated tmux server running Zsh with zsuggestion loaded,
//! drives keystrokes through tmux, and asserts on the rendered popup menu and
//! ZLE state. Tests are independent: each spins up its own tmux session so a
//! failure in one test does not cascade into others.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tempfile::tempdir;

/// A running tmux + Zsh session with zsuggestion loaded.
///
/// Drops the tmux server on cleanup.
struct ZleSession {
    server: String,
    binary: PathBuf,
    config: PathBuf,
    state: PathBuf,
    socket: PathBuf,
    zdotdir: PathBuf,
    temporary: tempfile::TempDir,
    state_dump: PathBuf,
    sync_file: PathBuf,
    escape_request: PathBuf,
}

impl ZleSession {
    /// Spawns a new tmux server, writes `.zshrc`, starts zsuggestion, and waits
    /// until the first popup appears after typing `zsu`.
    fn new() -> Self {
        if !command_exists("tmux") || !command_exists("zsh") {
            eprintln!("skipping ZLE integration test because tmux or zsh is unavailable");
            std::process::exit(0);
        }

        let binary = Path::new(env!("CARGO_BIN_EXE_zsuggestion")).to_path_buf();
        let binary_directory = binary.parent().unwrap().to_path_buf();
        let temporary = tempdir().unwrap();
        let state = temporary.path().join("state");
        let zdotdir = temporary.path().join("zsh");
        let helper_bin = temporary.path().join("bin");
        fs::create_dir_all(&state).unwrap();
        fs::create_dir_all(&zdotdir).unwrap();
        fs::create_dir_all(&helper_bin).unwrap();
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(&zdotdir, fs::Permissions::from_mode(0o700)).unwrap();

        let config = temporary.path().join("zsuggestion.toml");
        fs::write(&config, "[completion]\nmax_candidates = 24\n").unwrap();
        let socket = state.join("zsuggestion.sock");
        let state_dump = temporary.path().join("zle-state");
        let sync_file = temporary.path().join("zle-sync");
        let escape_request = temporary.path().join("zle-escape");

        let preview_file = temporary.path().join("preview-target.txt");
        let preview_file_two = temporary.path().join("preview-target.zzz.txt");
        fs::write(&preview_file, "preview-first-line\npreview-second-line\n").unwrap();
        fs::write(&preview_file_two, "preview-other-candidate\n").unwrap();
        fs::write(temporary.path().join("filesystem-alpha"), "alpha").unwrap();
        fs::write(temporary.path().join("filesystem-beta"), "beta").unwrap();
        fs::write(
            temporary.path().join("kitty.conf"),
            "font_family monospace\n",
        )
        .unwrap();
        fs::write(temporary.path().join("kitty.conf.bak"), "backup\n").unwrap();

        let fake_eza = helper_bin.join("eza");
        fs::write(
            &fake_eza,
            "#!/bin/sh\nprintf '\\033[31meza-alias-preview\\033[0m\\npreview-target.txt\\n'\n",
        )
        .unwrap();
        fs::set_permissions(&fake_eza, fs::Permissions::from_mode(0o755)).unwrap();

        let path = format!(
            "{}:{}:{}",
            helper_bin.display(),
            binary_directory.display(),
            std::env::var("PATH").unwrap_or_default()
        );

        let zshrc = build_zshrc(
            &binary,
            &helper_bin,
            &preview_file,
            &preview_file_two,
            &state_dump,
            &sync_file,
            &escape_request,
        );
        fs::write(zdotdir.join(".zshrc"), &zshrc).unwrap();

        let shell_environment = format!(
            "PATH={} ZDOTDIR={} ZSUGGESTION_CONFIG={} ZSUGGESTION_STATE_DIR={} ZSUGGESTION_SOCKET={}",
            shell_quote(&path),
            shell_quote(zdotdir.to_str().unwrap()),
            shell_quote(config.to_str().unwrap()),
            shell_quote(state.to_str().unwrap()),
            shell_quote(socket.to_str().unwrap()),
        );

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let server = format!("zsuggestion-ui-{}-{unique}", std::process::id());

        let status = Command::new("tmux")
            .args([
                "-L",
                &server,
                "-f",
                "/dev/null",
                "new-session",
                "-d",
                "-s",
                "test",
            ])
            .arg(format!("env {shell_environment} zsh -l"))
            .status()
            .unwrap();
        assert!(status.success(), "failed to start isolated tmux server");

        thread::sleep(Duration::from_millis(300));

        // Verify zsuggestion did not overwrite the user pane title.
        Command::new("tmux")
            .args([
                "-L",
                &server,
                "select-pane",
                "-t",
                "test:0.0",
                "-T",
                "user-title",
            ])
            .status()
            .unwrap();
        Command::new("tmux")
            .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0", ":"])
            .status()
            .unwrap();
        Command::new("tmux")
            .args(["-L", &server, "send-keys", "-t", "test:0.0", "Enter"])
            .status()
            .unwrap();
        thread::sleep(Duration::from_millis(100));
        let pane_title = Command::new("tmux")
            .args([
                "-L",
                &server,
                "display-message",
                "-p",
                "-t",
                "test:0.0",
                "#{pane_title}",
            ])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8(pane_title.stdout).unwrap().trim(),
            "user-title",
            "zsuggestion overwrote a user-owned tmux pane title"
        );

        // Type `zsu` and wait for the first popup.
        Command::new("tmux")
            .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0", "zsu"])
            .status()
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut capture = String::new();
        while Instant::now() < deadline {
            let output = Command::new("tmux")
                .args(["-L", &server, "capture-pane", "-p", "-e", "-t", "test:0.0"])
                .output()
                .unwrap();
            capture = String::from_utf8(output.stdout).unwrap();
            if capture.contains("Cmd") {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        assert!(
            capture.contains("Cmd"),
            "zsuggestion popup did not appear:\n{capture}"
        );
        assert!(
            capture.contains("\u{1b}[32mzsu"),
            "existing syntax highlight was lost after popup rendering:\n{capture:?}"
        );

        Self {
            server,
            binary,
            config,
            state,
            socket,
            zdotdir,
            temporary,
            state_dump,
            sync_file,
            escape_request,
        }
    }

    fn send_keys(&self, keys: &str) {
        Command::new("tmux")
            .args(["-L", &self.server, "send-keys", "-t", "test:0.0", keys])
            .status()
            .unwrap();
    }

    fn send_keys_literal(&self, text: &str) {
        Command::new("tmux")
            .args([
                "-L",
                &self.server,
                "send-keys",
                "-l",
                "-t",
                "test:0.0",
                text,
            ])
            .status()
            .unwrap();
    }

    fn wait_for_zle(&self) {
        wait_for_zle(&self.server, &self.sync_file);
    }

    fn capture_pane(&self, include_escape: bool) -> String {
        capture_pane(&self.server, include_escape)
    }

    fn dump_state(&self) -> String {
        dump_zle_state(&self.server, &self.state_dump);
        fs::read_to_string(&self.state_dump).unwrap_or_default()
    }

    fn read_state_fields(&self) -> Vec<String> {
        self.dump_state()
            .trim_end()
            .split('|')
            .map(|s| s.to_owned())
            .collect()
    }

    fn record_command(&self, command: &str, cwd: &Path, exit_code: i32, session: &str) {
        let status = Command::new(&self.binary)
            .args([
                "record",
                "--command",
                command,
                "--cwd",
                cwd.to_str().unwrap(),
                "--exit-code",
                exit_code.to_string().as_str(),
                "--session",
                session,
            ])
            .env("ZSUGGESTION_CONFIG", &self.config)
            .env("ZSUGGESTION_STATE_DIR", &self.state)
            .env("ZSUGGESTION_SOCKET", &self.socket)
            .status()
            .unwrap();
        assert!(status.success(), "failed to record command {command:?}");
    }

    fn resize_pane(&self, width: u32, height: u32) {
        Command::new("tmux")
            .args([
                "-L",
                &self.server,
                "resize-window",
                "-t",
                "test:0",
                "-x",
                width.to_string().as_str(),
                "-y",
                height.to_string().as_str(),
            ])
            .status()
            .unwrap();
    }

    fn new_window(&self, name: &str, env: &str) {
        Command::new("tmux")
            .args([
                "-L",
                &self.server,
                "new-window",
                "-d",
                "-t",
                "test",
                "-n",
                name,
            ])
            .arg(env)
            .status()
            .unwrap();
    }

    fn wait_for_pane_contains(&self, expected: &str) {
        wait_for_pane(&self.server, expected);
    }

    // Per-target operations for the secondary ASCII window.
    fn send_keys_target(&self, target: &str, keys: &str) {
        Command::new("tmux")
            .args(["-L", &self.server, "send-keys", "-t", target, keys])
            .status()
            .unwrap();
    }

    fn wait_for_zle_target(&self, target: &str) {
        wait_for_zle_target(&self.server, target, &self.sync_file);
    }

    fn capture_pane_target(&self, target: &str, include_escape: bool) -> String {
        capture_target(&self.server, target, include_escape)
    }

    fn dump_state_target(&self, target: &str) {
        dump_zle_state_target(&self.server, target, &self.state_dump);
    }

    fn resize_pane_target(&self, target: &str, width: u32, height: u32) {
        Command::new("tmux")
            .args([
                "-L",
                &self.server,
                "resize-window",
                "-t",
                target,
                "-x",
                width.to_string().as_str(),
                "-y",
                height.to_string().as_str(),
            ])
            .status()
            .unwrap();
    }
}

impl Drop for ZleSession {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["-L", &self.server, "kill-server"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = Command::new(&self.binary)
            .arg("stop")
            .env("ZSUGGESTION_CONFIG", &self.config)
            .env("ZSUGGESTION_STATE_DIR", &self.state)
            .env("ZSUGGESTION_SOCKET", &self.socket)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// Builds the `.zshrc` content with all test fixtures and zsuggestion loaded.
fn build_zshrc(
    binary: &Path,
    helper_bin: &Path,
    preview_file: &Path,
    preview_file_two: &Path,
    state_dump: &Path,
    sync_file: &Path,
    escape_request: &Path,
) -> String {
    format!(
        r#"export PATH={helper_bin}:$PATH
autoload -Uz add-zle-hook-widget compinit
compinit -u
autoload -Uz bracketed-paste-magic
zle -N bracketed-paste bracketed-paste-magic
_zsuggestion_test_history_up() {{
  BUFFER=history-arrow-up
  CURSOR=${{#BUFFER}}
}}
_zsuggestion_test_history_down() {{
  BUFFER=history-arrow-down
  CURSOR=${{#BUFFER}}
}}
zle -N _zsuggestion_test_history_up
zle -N _zsuggestion_test_history_down
bindkey '^[[A' _zsuggestion_test_history_up
bindkey '^[OA' _zsuggestion_test_history_up
bindkey '^[[B' _zsuggestion_test_history_down
bindkey '^[OB' _zsuggestion_test_history_down
_zsuggestion_test_highlight() {{
  region_highlight=( "${{(@)region_highlight:#*memo=foreign-test*}}" )
  [[ -n "$BUFFER" ]] && region_highlight+=("0 ${{#BUFFER}} fg=green memo=foreign-test")
}}
add-zle-hook-widget line-pre-redraw _zsuggestion_test_highlight
_zsuggestion_test_native() {{
  sleep 0.12
  if (( CURRENT > 2 )); then
    compadd next-value
  else
    compadd native/path/file native/second/file native/third/file
  fi
}}
compdef _zsuggestion_test_native zsuggestion-native-fixture
_zsuggestion_test_file() {{
  compadd {preview_file} {preview_file_two}
}}
compdef _zsuggestion_test_file zsuggestion-preview-fixture
_zsuggestion_test_scp() {{
  if [[ "$PREFIX" == zz* ]]; then
    compadd 'zzuser@example.com:/srv/app/file'
  else
    compadd file-alpha file-beta
  fi
}}
compdef _zsuggestion_test_scp scp
_zsuggestion_test_ssh() {{
  compadd alice@example.com
}}
compdef _zsuggestion_test_ssh ssh
_zsuggestion_test_long() {{
  compadd visible-first-option visible-second-option
}}
compdef _zsuggestion_test_long zsuggestion-command-with-a-very-long-name
alias ls=eza
eval "$({binary} init zsh)"
_zsuggestion_test_dump_state() {{
  _ZSUGGESTION_DUMP_SEQ=$(( ${{_ZSUGGESTION_DUMP_SEQ:-0}} + 1 ))
  if [[ -e {escape_request} ]]; then
    command rm -f -- {escape_request}
    _zsuggestion_escape
  fi
  print -r -- "$_ZSUGGESTION_MENU_ACTIVE|${{#_ZSUGGESTION_MENU_ACCEPTS}}|$_ZSUGGESTION_MENU_BUFFER|$BUFFER|${{_ZSUGGESTION_MENU_ACCEPTS[1]}}|$_ZSUGGESTION_MENU_INDEX|${{_ZSUGGESTION_MENU_DISPLAYS[$_ZSUGGESTION_MENU_INDEX]}}|$_ZSUGGESTION_FUZZY_ACTIVE|$_ZSUGGESTION_FUZZY_BASE|$_ZSUGGESTION_FUZZY_QUERY|$_ZSUGGESTION_PREVIEW_FD|$_ZSUGGESTION_PREVIEW_TICKS|$_ZSUGGESTION_PREVIEW_PATH|${{(j:;:)_ZSUGGESTION_PREVIEW_LINES}}|${{(j:;:)_ZSUGGESTION_MENU_DISPLAYS}}|${{POSTDISPLAY%%$'\n'*}}|${{(j:;:)_ZSUGGESTION_MENU_SOURCES}}|$_ZSUGGESTION_DUMP_SEQ" > {state_dump}.new && command mv -f {state_dump}.new {state_dump}
}}
_zsuggestion_test_sync() {{
  : > {sync_file}
}}
zle -N _zsuggestion_test_dump_state
zle -N _zsuggestion_test_sync
bindkey '^X^D' _zsuggestion_test_dump_state
bindkey '^G' _zsuggestion_test_sync
bindkey -M zsuggestion-fuzzy '^X^D' _zsuggestion_test_dump_state
bindkey -M zsuggestion-fuzzy '^G' _zsuggestion_test_sync
PROMPT='%# '
"#,
        binary = shell_quote(binary.to_str().unwrap()),
        helper_bin = shell_quote(helper_bin.to_str().unwrap()),
        preview_file = shell_quote(preview_file.to_str().unwrap()),
        preview_file_two = shell_quote(preview_file_two.to_str().unwrap()),
        state_dump = shell_quote(state_dump.to_str().unwrap()),
        sync_file = shell_quote(sync_file.to_str().unwrap()),
        escape_request = shell_quote(escape_request.to_str().unwrap())
    )
}

fn command_exists(name: &str) -> bool {
    Command::new(name)
        .arg("-V")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

fn wait_for_zle(server: &str, sync_file: &Path) {
    let _ = fs::remove_file(sync_file);
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        Command::new("tmux")
            .args(["-L", server, "send-keys", "-t", "test:0.0", "C-g"])
            .status()
            .unwrap();
        thread::sleep(Duration::from_millis(50));
        if sync_file.exists() {
            return;
        }
    }
    panic!("ZLE did not become ready:\n{}", capture_pane(server, false));
}

fn dump_zle_state(server: &str, state_dump: &Path) {
    let read_seq = |path: &Path| -> Option<String> {
        let content = fs::read_to_string(path).ok()?;
        let fields: Vec<&str> = content.trim_end().split('|').collect();
        if fields.len() < 18 || fields[17].is_empty() {
            return None;
        }
        Some(fields[17].to_owned())
    };
    let before = read_seq(state_dump);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        Command::new("tmux")
            .args(["-L", server, "send-keys", "-t", "test:0.0", "C-x", "C-d"])
            .status()
            .unwrap();
        for _ in 0..40 {
            thread::sleep(Duration::from_millis(25));
            let current = read_seq(state_dump);
            if current.is_some() && current != before {
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "zle state dump never refreshed: {}",
            state_dump.display()
        );
    }
}

fn capture_pane(server: &str, include_escape_sequences: bool) -> String {
    let mut command = Command::new("tmux");
    command.args(["-L", server, "capture-pane", "-p"]);
    if include_escape_sequences {
        command.arg("-e");
    }
    let output = command.args(["-t", "test:0.0"]).output().unwrap();
    String::from_utf8(output.stdout).unwrap()
}

fn capture_target(server: &str, target: &str, include_escape: bool) -> String {
    let mut command = Command::new("tmux");
    command.args(["-L", server, "capture-pane", "-p"]);
    if include_escape {
        command.arg("-e");
    }
    let output = command.args(["-t", target]).output().unwrap();
    String::from_utf8(output.stdout).unwrap()
}

fn wait_for_pane(server: &str, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut capture = String::new();
    while Instant::now() < deadline {
        capture = capture_pane(server, false);
        if capture.contains(expected) {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("pane did not contain {expected:?}:\n{capture}");
}

fn dump_zle_state_target(server: &str, target: &str, state_dump: &Path) {
    let read_seq = |path: &Path| -> Option<String> {
        let content = fs::read_to_string(path).ok()?;
        let fields: Vec<&str> = content.trim_end().split('|').collect();
        if fields.len() < 18 || fields[17].is_empty() {
            return None;
        }
        Some(fields[17].to_owned())
    };
    let before = read_seq(state_dump);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        Command::new("tmux")
            .args(["-L", server, "send-keys", "-t", target, "C-x", "C-d"])
            .status()
            .unwrap();
        for _ in 0..40 {
            thread::sleep(Duration::from_millis(25));
            let current = read_seq(state_dump);
            if current.is_some() && current != before {
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "zle state dump never refreshed: {}",
            state_dump.display()
        );
    }
}

fn wait_for_zle_target(server: &str, target: &str, sync_file: &Path) {
    let _ = fs::remove_file(sync_file);
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        Command::new("tmux")
            .args(["-L", server, "send-keys", "-t", target, "C-g"])
            .status()
            .unwrap();
        thread::sleep(Duration::from_millis(50));
        if sync_file.exists() {
            return;
        }
    }
    panic!("ZLE did not become ready in {target}");
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn cursor_x(server: &str, target: &str) -> usize {
    let output = Command::new("tmux")
        .args([
            "-L",
            server,
            "display-message",
            "-p",
            "-t",
            target,
            "#{cursor_x}",
        ])
        .output()
        .unwrap();
    String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn popup_appears_and_accept_candidate() {
    let session = ZleSession::new();

    // Accept the highlighted candidate with Ctrl-Space.
    session.send_keys("C-Space");

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut accepted_capture = String::new();
    while Instant::now() < deadline {
        accepted_capture = session.capture_pane(true);
        if accepted_capture.contains("\u{1b}[32mzsuggestion") {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        accepted_capture.contains("\u{1b}[32mzsuggestion"),
        "accepted command was not fully highlighted:\n{accepted_capture:?}"
    );

    // Key bindings are preserved after zsuggestion loads.
    session.send_keys("C-c");
    session.wait_for_zle();
    let binding_file = session.temporary.path().join("enter-binding");
    session.send_keys_literal(&format!(
        "{{ bindkey '^M'; bindkey '^I'; bindkey '^[[Z'; }} > {}",
        shell_quote(binding_file.to_str().unwrap())
    ));
    session.send_keys("Enter");

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && !binding_file.exists() {
        thread::sleep(Duration::from_millis(50));
    }
    let binding = fs::read_to_string(&binding_file).unwrap_or_else(|error| {
        panic!(
            "Enter did not execute bindkey: {error}\n{}",
            session.capture_pane(false)
        )
    });
    assert!(binding.contains(r#""^M" accept-line"#));
    assert!(binding.contains(r#""^I" zsuggestion-tab"#));
    assert!(binding.contains("zsuggestion-shift-tab"));
}

#[test]
fn path_menu_arrow_keys_and_escape() {
    let session = ZleSession::new();

    session.send_keys_literal("cd ");
    thread::sleep(Duration::from_millis(500));
    session.dump_state();
    let reordered_state = session.read_state_fields();
    assert_eq!(reordered_state[0], "1", "cd path menu was not active");
    assert!(
        reordered_state[1].parse::<usize>().unwrap() > 1,
        "cd path menu did not contain multiple candidates: {reordered_state:?}"
    );
    assert_eq!(
        reordered_state[5], "1",
        "async provider merging moved the menu away from row 1: {reordered_state:?}"
    );

    session.send_keys("Down");
    thread::sleep(Duration::from_millis(150));
    session.dump_state();
    let arrow_down_fields = session.read_state_fields();
    assert_eq!(arrow_down_fields[0], "1", "arrow key closed the popup menu");
    assert_eq!(
        arrow_down_fields[5], "2",
        "Down did not move the popup selection while open"
    );

    session.send_keys("Up");
    thread::sleep(Duration::from_millis(150));
    session.dump_state();
    let arrow_up_fields = session.read_state_fields();
    assert_eq!(
        arrow_up_fields[5], "1",
        "Up did not move the popup selection back"
    );

    // Escape dismisses the popup.
    fs::write(&session.escape_request, "").unwrap();
    session.dump_state();
    let escaped_fields = session.read_state_fields();
    assert_eq!(escaped_fields[0], "0", "escape did not dismiss the popup");
    assert_eq!(
        escaped_fields[3], "cd ",
        "escape changed the command buffer"
    );

    // History up/down work after escape.
    session.send_keys("Up");
    thread::sleep(Duration::from_millis(350));
    session.dump_state();
    let history_up_fields = session.read_state_fields();
    assert_eq!(history_up_fields[0], "0");
    assert_eq!(history_up_fields[3], "history-arrow-up");

    session.send_keys("Down");
    session.dump_state();
    let history_down_fields = session.read_state_fields();
    assert_eq!(history_down_fields[0], "0");
    assert_eq!(history_down_fields[3], "history-arrow-down");

    session.send_keys("C-c");
    session.wait_for_zle();
}

#[test]
fn history_is_bottom_tier() {
    let session = ZleSession::new();

    session.record_command(
        "cargo release patch --execute",
        session.temporary.path(),
        0,
        "history-priority-test",
    );
    session.send_keys_literal("cargo release patch ");

    let deadline = Instant::now() + Duration::from_secs(10);
    let history_priority_state = loop {
        assert!(
            Instant::now() < deadline,
            "history priority menu never appeared:\n{}",
            session.capture_pane(false)
        );
        let fields: Vec<String> = session.read_state_fields();
        if fields[0] == "1"
            && fields[14]
                .split(';')
                .any(|display| display == "cargo release patch --execute")
        {
            break fields.join("|");
        }
        thread::sleep(Duration::from_millis(50));
    };
    let fields: Vec<_> = history_priority_state.split('|').collect();
    let priority_sources: Vec<_> = fields[16].split(';').collect();
    assert_eq!(
        priority_sources.last().copied(),
        Some("history"),
        "recorded history is no longer the bottom tier: {history_priority_state}"
    );
    assert!(
        fields[16].split(';').any(|source| source == "filesystem"),
        "filesystem fixture did not compete with history: {history_priority_state}"
    );

    session.send_keys("C-c");
    session.wait_for_zle();
}

#[test]
fn long_command_rows_preserve_suffixes() {
    let session = ZleSession::new();

    session.send_keys_literal("zsuggestion-command-with-a-very-long-name v");
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut long_candidate_capture = String::new();
    while Instant::now() < deadline {
        long_candidate_capture = session.capture_pane(false);
        if long_candidate_capture.contains("… isible-first-option")
            && long_candidate_capture.contains("… isible-second-option")
        {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        long_candidate_capture.contains("… isible-first-option")
            && long_candidate_capture.contains("… isible-second-option"),
        "long completion rows did not preserve their distinguishing suffixes:\n{long_candidate_capture}"
    );

    session.send_keys("C-c");
    session.wait_for_zle();
}

#[test]
fn bracketed_paste_does_not_replace_line() {
    let session = ZleSession::new();

    session.send_keys_literal("echo before");
    thread::sleep(Duration::from_millis(100));
    session.send_keys_literal("\u{1b}[200~ pasted\u{1b}[201~");
    session.send_keys_literal(" after");
    thread::sleep(Duration::from_millis(500));
    session.dump_state();
    let fields = session.read_state_fields();
    assert_eq!(
        fields[3], "echo before pasted after",
        "typing after bracketed paste replaced the existing line"
    );

    session.send_keys("C-c");
    session.wait_for_zle();
}

#[test]
fn native_completion_tab_fill_and_ssh_scp() {
    let session = ZleSession::new();

    // Native completion is debounced, not synchronous.
    session.send_keys_literal("zsuggestion-native-fixture na");
    let immediate_capture = session.capture_pane(false);
    assert!(
        !immediate_capture.contains("native/path/file"),
        "native completion ran synchronously instead of waiting for the debounce"
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut native_capture = String::new();
    while Instant::now() < deadline {
        native_capture = session.capture_pane(false);
        if native_capture.contains("zsuggestion-native-fixture native/path/file") {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        native_capture.contains("zsuggestion-native-fixture native/path/file")
            && native_capture.contains("Zsh completion"),
        "debounced native Zsh candidate was absent:\n{native_capture}"
    );

    // Candidates are retained while typing the next character.
    session.send_keys_literal("t");
    let retained_capture = session.capture_pane(false);
    assert!(
        retained_capture.contains("Native")
            && retained_capture.contains("zsuggestion-native-fixture native/path/file"),
        "matching candidates flashed off while the next request was pending:\n{retained_capture}"
    );

    // Shift-Tab selects the next candidate and ghost text reflects it.
    session.send_keys("C-n");
    session.send_keys("C-n");
    session.send_keys("BTab");
    session.dump_state();
    let shifted_state = session.read_state_fields();
    assert!(
        shifted_state
            .iter()
            .any(|s| s == "|2|zsuggestion-native-fixture native/second/file"),
        "Shift-Tab did not select the next candidate: {shifted_state:?}"
    );
    let shifted_fields: Vec<_> = shifted_state;
    assert_eq!(
        shifted_fields[15], "ive/second/file",
        "selected candidate was not reflected in ghost text"
    );

    // Ordinary typing updates the buffer after selection.
    session.send_keys_literal("i");
    session.dump_state();
    let typed_state = session.read_state_fields();
    assert!(
        typed_state
            .iter()
            .any(|s| s == "|zsuggestion-native-fixture nati|zsuggestion-native-fixture nati|"),
        "ordinary typing did not update the buffer after selection: {typed_state:?}"
    );

    // Backspace resumes completion from the edited buffer.
    session.send_keys("BSpace");
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        native_capture = session.capture_pane(false);
        if native_capture.contains("zsuggestion-native-fixture native/path/file") {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        native_capture.contains("zsuggestion-native-fixture native/path/file"),
        "Backspace did not resume completion from the edited buffer:\n{native_capture}"
    );

    // Tab fills the selected candidate and native completion continues.
    session.send_keys("C-n");
    session.dump_state();
    let before_fill_fields = session.read_state_fields();
    let selected_index: usize = before_fill_fields[5].parse().unwrap();
    let selected_display = before_fill_fields[14]
        .split(';')
        .nth(selected_index - 1)
        .unwrap()
        .to_owned();
    session.send_keys("Tab");
    session.dump_state();
    let after_tab_fields = session.read_state_fields();
    assert_eq!(
        after_tab_fields[3],
        format!("{selected_display} "),
        "Tab did not fill the selected candidate"
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut follow_up_state = String::new();
    while Instant::now() < deadline {
        session.dump_state();
        follow_up_state = session.read_state_fields().join("|");
        if follow_up_state.contains("next-value") {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        follow_up_state.contains("next-value"),
        "native completion did not continue after accepting a complete match"
    );

    session.send_keys("C-c");
    session.wait_for_zle();

    // SSH candidate.
    session.send_keys_literal("ssh ali");
    session.wait_for_pane_contains("ssh alice@example.com");
    session.send_keys("Tab");
    session.dump_state();
    let ssh_fields = session.read_state_fields();
    assert_eq!(
        ssh_fields[3], "ssh alice@example.com ",
        "Tab did not fill the SSH candidate"
    );

    session.send_keys("C-c");
    session.wait_for_zle();

    // SCP remote destination candidate.
    session.send_keys_literal("scp zzu");
    session.wait_for_pane_contains("scp zzuser@example.com:/srv/app/file");
    session.send_keys("Tab");
    session.dump_state();
    let remote_fields = session.read_state_fields();
    assert_eq!(
        remote_fields[3], "scp zzuser@example.com:/srv/app/file ",
        "Tab did not fill the remote destination candidate"
    );

    session.send_keys("C-c");
    session.wait_for_zle();
}

#[test]
fn filesystem_unique_candidate_and_exact_path() {
    let session = ZleSession::new();

    let scp_path_prefix = format!("scp -r {}/filesystem-", session.temporary.path().display());
    session.send_keys_literal(&scp_path_prefix);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut scp_capture = String::new();
    while Instant::now() < deadline {
        scp_capture = session.capture_pane(false);
        if scp_capture.matches("File").count() >= 2 {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        scp_capture.matches("File").count() >= 2,
        "generic filesystem candidates were absent for scp:\n{scp_capture}"
    );
    session.dump_state();
    let scp_before_tab = session.read_state_fields();
    assert!(
        scp_before_tab.contains(&format!("|{scp_path_prefix}|{scp_path_prefix}|")),
        "zsuggestion did not retain the scp path prefix: {scp_before_tab:?}"
    );

    session.send_keys("Tab");
    session.dump_state();
    let scp_after_tab = session.read_state_fields();
    let scp_selected: usize = scp_after_tab[5].parse().unwrap();
    assert_ne!(scp_selected, 0, "menu was not active after the fill");
    assert!(
        scp_after_tab[3].starts_with(&scp_path_prefix),
        "ambiguous path Tab did not fill the selected row"
    );

    // Narrow to a unique candidate.
    session.send_keys("C-c");
    session.wait_for_zle();
    session.send_keys_literal(&scp_path_prefix);
    session.send_keys_literal("a");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(
            Instant::now() < deadline,
            "narrowed filesystem candidate was absent:\n{}",
            session.capture_pane(false)
        );
        session.dump_state();
        let unique_fields = session.read_state_fields();
        if unique_fields[1].parse::<usize>().map(|c| c >= 1) == Ok(true) {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    session.send_keys("Tab");
    session.dump_state();
    let scp_unique_path = session.read_state_fields().join("|");
    let prefix_without_suffix = &scp_path_prefix[..scp_path_prefix.len() - "filesystem-".len()];
    assert!(
        scp_unique_path.contains(&format!(
            "|{prefix_without_suffix}filesystem-alpha |{prefix_without_suffix}filesystem-alpha |"
        )),
        "Tab did not accept the unique filesystem candidate: {scp_unique_path}"
    );

    session.send_keys("C-c");
    session.wait_for_zle();

    // Exact file path fill.
    let kitty_prefix = format!("nv {}/kitty.con", session.temporary.path().display());
    session.send_keys_literal(&kitty_prefix);
    session.wait_for_pane_contains("f.bak");
    session.dump_state();
    let kitty_fields = session.read_state_fields();
    let kitty_index: usize = kitty_fields[5].parse().unwrap();
    let kitty_selected = kitty_fields[14]
        .split(';')
        .nth(kitty_index - 1)
        .unwrap()
        .to_owned();
    session.send_keys("Tab");
    session.dump_state();
    let exact_path_fields = session.read_state_fields();
    assert_eq!(
        exact_path_fields[3],
        format!("{kitty_selected} "),
        "Tab did not fill the selected file candidate"
    );

    session.send_keys("C-c");
    session.wait_for_zle();
}

#[test]
fn reload_zshrc_restarts_ticker() {
    let session = ZleSession::new();

    session.send_keys("C-c");
    session.wait_for_zle();
    let source_command = format!(
        "zle -D _zsuggestion-native-space; source {}",
        shell_quote(session.zdotdir.join(".zshrc").to_str().unwrap())
    );
    session.send_keys_literal(&source_command);
    session.send_keys("Enter");
    thread::sleep(Duration::from_millis(300));

    let rebound_file = session.temporary.path().join("reloaded-bindings");
    let rebound_command = format!(
        "{{ bindkey '^I'; bindkey '^[[Z'; bindkey '^@'; }} > {}",
        shell_quote(rebound_file.to_str().unwrap())
    );
    session.send_keys_literal(&rebound_command);
    session.send_keys("Enter");

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && !rebound_file.exists() {
        thread::sleep(Duration::from_millis(20));
    }
    let rebound = fs::read_to_string(&rebound_file).unwrap();
    assert!(rebound.contains(r#""^I" zsuggestion-tab"#));
    assert!(rebound.contains("zsuggestion-shift-tab"));
    assert!(rebound.contains(r#""^@" zsuggestion-complete"#));

    session.send_keys_literal("zsu");
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut reloaded_capture = String::new();
    while Instant::now() < deadline {
        reloaded_capture = session.capture_pane(false);
        if reloaded_capture.contains("Cmd") {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        reloaded_capture.contains("Cmd"),
        "zsuggestion did not restart its ticker after .zshrc was sourced:\n{reloaded_capture}"
    );

    session.send_keys("C-c");
    session.wait_for_zle();
}

#[test]
fn fuzzy_mode_ghost_and_escape() {
    let session = ZleSession::new();

    session.record_command(
        "echo fuzzy-history-target",
        session.temporary.path(),
        0,
        "fuzzy-test",
    );
    session.record_command(
        "echo fuzzy-history-target-secondary",
        session.temporary.path(),
        0,
        "fuzzy-test",
    );
    let fuzzy_execute_command = "print -r -- fuzzy-enter-executed > /tmp/fuzzy-executed-test";
    session.record_command(
        fuzzy_execute_command,
        session.temporary.path(),
        0,
        "fuzzy-execute-test",
    );

    session.send_keys("C-c");
    session.wait_for_zle();
    session.send_keys_literal("  fzht");
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut fuzzy_capture = String::new();
    while Instant::now() < deadline {
        fuzzy_capture = session.capture_pane(false);
        if fuzzy_capture.contains("echo fuzzy-history-target") {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        fuzzy_capture.contains("echo fuzzy-history-target"),
        "inline fuzzy history result was absent:\n{fuzzy_capture}"
    );
    session.dump_state();
    let fuzzy_fields = session.read_state_fields();
    assert_eq!(fuzzy_fields[3], "fzht");
    assert_eq!(fuzzy_fields[7], "1");
    assert_eq!(fuzzy_fields[8], "");
    assert_eq!(fuzzy_fields[9], "fzht");
    assert_eq!(
        fuzzy_fields[15],
        format!("  → {}", fuzzy_fields[6]),
        "selected fuzzy candidate was not reflected in ghost text"
    );
    assert_eq!(
        cursor_x(&session.server, "test:0.0"),
        6,
        "fuzzy query did not retain ZLE's real cursor"
    );
    assert!(
        fuzzy_fields[1].parse::<usize>().unwrap() >= 2,
        "fuzzy fixture did not produce scrollable candidates"
    );

    // Scrolling updates ghost text.
    session.send_keys("C-n");
    session.dump_state();
    let scrolled_fields = session.read_state_fields();
    assert_ne!(scrolled_fields[6], fuzzy_fields[6]);
    assert_eq!(scrolled_fields[8], "");
    assert_eq!(
        scrolled_fields[15],
        format!("  → {}", scrolled_fields[6]),
        "scrolling did not update fuzzy ghost text"
    );

    // Escape dismisses fuzzy mode.
    fs::write(&session.escape_request, "").unwrap();
    session.dump_state();
    let escaped_fields = session.read_state_fields();
    assert_eq!(
        escaped_fields[3], "",
        "fuzzy escape state: {escaped_fields:?}"
    );
    assert_eq!(escaped_fields[7], "0");
    assert_eq!(escaped_fields[8], "");
    assert_eq!(escaped_fields[9], "");

    // Re-enter fuzzy and accept with Tab.
    session.send_keys_literal("  fzht");
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        fuzzy_capture = session.capture_pane(false);
        if fuzzy_capture.contains("echo fuzzy-history-target") {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    session.send_keys("Tab");
    session.dump_state();
    let accepted_fields = session.read_state_fields();
    assert!(
        accepted_fields[3].starts_with("echo fuzzy-history-target"),
        "Tab did not accept fuzzy candidate: {accepted_fields:?}"
    );
    assert_eq!(accepted_fields[7], "0");

    session.send_keys("C-c");
    session.wait_for_zle();

    // Fuzzy Enter executes the selected command.
    session.send_keys_literal("  fuzzy-enter-executed");
    session.wait_for_pane_contains("Enter run");
    session.send_keys("Enter");
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && !Path::new("/tmp/fuzzy-executed-test").exists() {
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        fs::read_to_string("/tmp/fuzzy-executed-test").unwrap(),
        "fuzzy-enter-executed\n",
        "fuzzy Enter selected the command without executing it"
    );
    session.wait_for_zle();

    // Interrupted fuzzy queries restore base.
    session.send_keys_literal("echo first  stale");
    session.dump_state();
    let first_fields = session.read_state_fields();
    assert_eq!(first_fields[7], "1");
    assert_eq!(first_fields[8], "echo first");
    assert_eq!(first_fields[9], "stale");
    session.send_keys("C-c");
    session.wait_for_zle();

    session.send_keys_literal("echo second  fresh");
    session.dump_state();
    let second_fields = session.read_state_fields();
    assert_eq!(second_fields[7], "1");
    assert_eq!(second_fields[8], "echo second");
    assert_eq!(second_fields[9], "fresh");
    fs::write(&session.escape_request, "").unwrap();
    session.dump_state();
    let escaped_fields = session.read_state_fields();
    assert_eq!(escaped_fields[3], "echo second");
    assert_eq!(escaped_fields[7], "0");
    assert_eq!(escaped_fields[8], "");
    assert_eq!(escaped_fields[9], "");

    session.send_keys("C-c");
    session.wait_for_zle();
}

#[test]
fn command_and_file_previews() {
    let session = ZleSession::new();

    session.send_keys("C-c");
    session.wait_for_zle();

    // Wide terminal: command preview for `ls`.
    session.resize_pane(140, 40);
    thread::sleep(Duration::from_millis(50));
    session.send_keys_literal("echo fuzzy");
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut history_capture = String::new();
    while Instant::now() < deadline {
        history_capture = session.capture_pane(false);
        if history_capture.contains("echo fuzzy-history-target") {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(history_capture.contains("echo fuzzy-history-target"));
    assert!(
        !history_capture.contains("Preview:"),
        "history-only suggestions opened an unnecessary preview:\n{history_capture}"
    );

    session.send_keys("C-c");
    session.wait_for_zle();
    session.send_keys_literal("ls ");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut command_preview_capture = String::new();
    while Instant::now() < deadline {
        command_preview_capture = session.capture_pane(true);
        if command_preview_capture.contains("eza-alias-preview")
            && command_preview_capture.contains("\u{1b}[31meza-alias-preview")
            && command_preview_capture.contains("preview-target.txt")
            && command_preview_capture.contains("Preview:")
        {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        command_preview_capture.contains("eza-alias-preview")
            && command_preview_capture.contains("\u{1b}[31meza-alias-preview")
            && command_preview_capture.contains("preview-target.txt")
            && command_preview_capture.contains("Preview:"),
        "safe eza-backed ls command preview was absent:\n{command_preview_capture}"
    );

    session.send_keys("C-c");
    session.wait_for_zle();

    // File preview: switch rows and the preview follows.
    let preview_file = session.temporary.path().join("preview-target.txt");
    let preview_prefix = preview_file.to_str().unwrap().strip_suffix("txt").unwrap();
    session.send_keys_literal(&format!("zsuggestion-preview-fixture {preview_prefix}"));
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut preview_capture = String::new();
    while Instant::now() < deadline {
        preview_capture = session.capture_pane(false);
        if preview_capture.contains("preview-first-line") {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    session.dump_state();
    let preview_state = session.read_state_fields().join("|");
    assert!(
        preview_capture.contains("preview-first-line") && preview_capture.contains("Preview:"),
        "lazy wide-terminal file preview was absent; state was {preview_state}:\n{preview_capture}"
    );

    session.send_keys("C-n");
    thread::sleep(Duration::from_millis(20));
    let switching_preview = session.capture_pane(false);
    assert!(
        !switching_preview.contains("preview-first-line"),
        "the previous preview remained visible while switching rows"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut second_preview = String::new();
    while Instant::now() < deadline {
        second_preview = session.capture_pane(false);
        if second_preview.contains("preview-other-candidate") {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        second_preview.contains("preview-other-candidate")
            && !second_preview.contains("preview-first-line"),
        "the second preview did not replace the first cleanly"
    );

    session.resize_pane(80, 24);
    session.send_keys("C-c");
    session.wait_for_zle();
}

#[test]
fn ascii_non_utf8_fallback_menu() {
    let session = ZleSession::new();

    let path = format!(
        "{}:{}:{}",
        session.temporary.path().join("bin").display(),
        Path::new(env!("CARGO_BIN_EXE_zsuggestion"))
            .parent()
            .unwrap()
            .display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let ascii_shell = format!(
        "env LC_ALL=C LANG=C PATH={} ZDOTDIR={} ZSUGGESTION_CONFIG={} ZSUGGESTION_STATE_DIR={} ZSUGGESTION_SOCKET={} zsh -l",
        shell_quote(&path),
        shell_quote(session.zdotdir.to_str().unwrap()),
        shell_quote(session.config.to_str().unwrap()),
        shell_quote(session.state.to_str().unwrap()),
        shell_quote(session.socket.to_str().unwrap()),
    );
    session.new_window("ascii", &ascii_shell);
    wait_for_zle_target(&session.server, "test:ascii.0", &session.sync_file);

    session.send_keys_target("test:ascii.0", "zsu");
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut ascii_capture = String::new();
    while Instant::now() < deadline {
        ascii_capture = session.capture_pane_target("test:ascii.0", false);
        if ascii_capture.contains("Cmd") && ascii_capture.contains("zsuggestion") {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        ascii_capture.contains("Cmd") && ascii_capture.contains("| $"),
        "ASCII fallback menu was absent:\n{ascii_capture}"
    );
    assert!(
        !ascii_capture.contains("<e2>") && !ascii_capture.contains('╭'),
        "non-UTF-8 menu still contained Unicode rendering:\n{ascii_capture}"
    );
    assert_eq!(cursor_x(&session.server, "test:ascii.0"), 5);

    session.send_keys_target("test:ascii.0", "BSpace");
    thread::sleep(Duration::from_millis(50));
    assert_eq!(cursor_x(&session.server, "test:ascii.0"), 4);
    session.dump_state_target("test:ascii.0");
    let ascii_state = fs::read_to_string(&session.state_dump).unwrap();
    let ascii_fields: Vec<_> = ascii_state.trim_end().split('|').collect();
    assert_eq!(ascii_fields[3], "zs");

    session.send_keys_target("test:ascii.0", "u");
    thread::sleep(Duration::from_millis(150));
    session.resize_pane_target("test:ascii", 20, 15);
    session.send_keys_target("test:ascii.0", "C-l");
    thread::sleep(Duration::from_millis(100));
    let narrow_capture = session.capture_pane_target("test:ascii.0", false);
    assert!(
        narrow_capture.contains("zsuggestion") && !narrow_capture.contains("+-"),
        "narrow terminal did not retain a compact suggestion:\n{narrow_capture}"
    );
    assert_eq!(cursor_x(&session.server, "test:ascii.0"), 5);

    session.send_keys_target("test:ascii.0", "C-c");
    session.wait_for_zle_target("test:ascii.0");
}
