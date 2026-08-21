use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tempfile::tempdir;

#[test]
fn popup_preserves_highlights_and_shell_bindings() {
    if !command_exists("tmux") || !command_exists("zsh") {
        eprintln!("skipping ZLE integration test because tmux or zsh is unavailable");
        return;
    }

    let binary = Path::new(env!("CARGO_BIN_EXE_zsuggestion"));
    let binary_directory = binary.parent().unwrap();
    let temporary = tempdir().unwrap();
    let state = temporary.path().join("state");
    let zdotdir = temporary.path().join("zsh");
    let helper_bin = temporary.path().join("bin");
    fs::create_dir(&state).unwrap();
    fs::create_dir(&zdotdir).unwrap();
    fs::create_dir(&helper_bin).unwrap();
    fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(&zdotdir, fs::Permissions::from_mode(0o700)).unwrap();

    let config = temporary.path().join("zsuggestion.toml");
    let socket = state.join("zsuggestion.sock");
    let state_dump = temporary.path().join("zle-state");
    let sync_file = temporary.path().join("zle-sync");
    let escape_request = temporary.path().join("zle-escape");
    let fuzzy_execute_file = temporary.path().join("fuzzy-executed");
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
    let zshrc = format!(
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
  if [[ -e {escape_request} ]]; then
    command rm -f -- {escape_request}
    _zsuggestion_escape
  fi
  print -r -- "$_ZSUGGESTION_MENU_ACTIVE|${{#_ZSUGGESTION_MENU_ACCEPTS}}|$_ZSUGGESTION_MENU_BUFFER|$BUFFER|${{_ZSUGGESTION_MENU_ACCEPTS[1]}}|$_ZSUGGESTION_MENU_INDEX|${{_ZSUGGESTION_MENU_DISPLAYS[$_ZSUGGESTION_MENU_INDEX]}}|$_ZSUGGESTION_FUZZY_ACTIVE|$_ZSUGGESTION_FUZZY_BASE|$_ZSUGGESTION_FUZZY_QUERY|$_ZSUGGESTION_PREVIEW_FD|$_ZSUGGESTION_PREVIEW_TICKS|$_ZSUGGESTION_PREVIEW_PATH|${{(j:;:)_ZSUGGESTION_PREVIEW_LINES}}|${{(j:;:)_ZSUGGESTION_MENU_DISPLAYS}}|${{POSTDISPLAY%%$'\n'*}}|${{(j:;:)_ZSUGGESTION_MENU_SOURCES}}" > {state_dump}
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
    );
    fs::write(zdotdir.join(".zshrc"), zshrc).unwrap();
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
    let mut guard = ServerGuard {
        server: server.clone(),
        binary: binary.to_owned(),
        config: config.clone(),
        state: state.clone(),
        socket: socket.clone(),
        started: false,
    };

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
    guard.started = true;

    thread::sleep(Duration::from_millis(300));
    let status = Command::new("tmux")
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
    assert!(status.success(), "failed to set the test pane title");
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
    let status = Command::new("tmux")
        .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0", "zsu"])
        .status()
        .unwrap();
    assert!(status.success(), "failed to type into isolated Zsh");

    let deadline = Instant::now() + Duration::from_secs(4);
    let mut capture = String::new();
    while Instant::now() < deadline {
        let output = Command::new("tmux")
            .args(["-L", &server, "capture-pane", "-p", "-e", "-t", "test:0.0"])
            .output()
            .unwrap();
        capture = String::from_utf8(output.stdout).unwrap();
        if capture.contains('╭') {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    assert!(
        capture.contains('╭'),
        "zsuggestion popup did not appear:\n{capture}"
    );
    assert!(
        capture.contains("\u{1b}[32mzsu"),
        "existing syntax highlight was lost after popup rendering:\n{capture:?}"
    );

    let status = Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-Space"])
        .status()
        .unwrap();
    assert!(status.success(), "failed to accept the selected candidate");

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut accepted_capture = String::new();
    while Instant::now() < deadline {
        accepted_capture = capture_pane(&server, true);
        if accepted_capture.contains("\u{1b}[32mzsuggestion") {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        accepted_capture.contains("\u{1b}[32mzsuggestion"),
        "accepted command was not fully highlighted:\n{accepted_capture:?}"
    );

    let binding_file = temporary.path().join("enter-binding");
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-c"])
        .status()
        .unwrap();
    wait_for_zle(&server, &sync_file);
    let binding_command = format!(
        "{{ bindkey '^M'; bindkey '^I'; bindkey '^[[Z'; }} > {}",
        shell_quote(binding_file.to_str().unwrap())
    );
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0"])
        .arg(binding_command)
        .status()
        .unwrap();
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "Enter"])
        .status()
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && !binding_file.exists() {
        thread::sleep(Duration::from_millis(50));
    }
    let binding = fs::read_to_string(&binding_file).unwrap_or_else(|error| {
        panic!(
            "Enter did not execute bindkey: {error}\n{}",
            capture_pane(&server, false)
        )
    });
    assert!(binding.contains(r#""^M" accept-line"#));
    assert!(binding.contains(r#""^I" zsuggestion-tab"#));
    assert!(binding.contains("zsuggestion-shift-tab"));

    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0", "cd "])
        .status()
        .unwrap();
    thread::sleep(Duration::from_millis(500));
    dump_zle_state(&server);
    let reordered_state = fs::read_to_string(&state_dump).unwrap();
    let reordered_fields: Vec<_> = reordered_state.trim_end().split('|').collect();
    assert_eq!(reordered_fields[0], "1", "cd path menu was not active");
    assert!(
        reordered_fields[1].parse::<usize>().unwrap() > 1,
        "cd path menu did not contain multiple candidates: {reordered_state:?}"
    );
    assert_eq!(
        reordered_fields[5], "1",
        "async provider merging moved the menu away from row 1: {reordered_state:?}"
    );
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "Up"])
        .status()
        .unwrap();
    thread::sleep(Duration::from_millis(350));
    dump_zle_state(&server);
    let history_up_state = fs::read_to_string(&state_dump).unwrap();
    let history_up_fields: Vec<_> = history_up_state.trim_end().split('|').collect();
    assert_eq!(history_up_fields[0], "0");
    assert_eq!(history_up_fields[3], "history-arrow-up");
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "Down"])
        .status()
        .unwrap();
    dump_zle_state(&server);
    let history_down_state = fs::read_to_string(&state_dump).unwrap();
    let history_down_fields: Vec<_> = history_down_state.trim_end().split('|').collect();
    assert_eq!(history_down_fields[0], "0");
    assert_eq!(history_down_fields[3], "history-arrow-down");
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-c"])
        .status()
        .unwrap();
    wait_for_zle(&server, &sync_file);

    let status = Command::new(binary)
        .args([
            "record",
            "--command",
            "cargo release patch --execute",
            "--cwd",
            temporary.path().to_str().unwrap(),
            "--exit-code",
            "0",
            "--session",
            "history-priority-test",
        ])
        .env("ZSUGGESTION_CONFIG", &config)
        .env("ZSUGGESTION_STATE_DIR", &state)
        .env("ZSUGGESTION_SOCKET", &socket)
        .status()
        .unwrap();
    assert!(status.success(), "failed to seed priority history");
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0"])
        .arg("cargo release patch ")
        .status()
        .unwrap();
    wait_for_pane(&server, "cargo release patch --execute");
    dump_zle_state(&server);
    let history_priority_state = fs::read_to_string(&state_dump).unwrap();
    let history_priority_fields: Vec<_> = history_priority_state.trim_end().split('|').collect();
    assert_eq!(history_priority_fields[6], "cargo release patch --execute");
    assert!(
        history_priority_fields[16]
            .split(';')
            .any(|source| source == "filesystem"),
        "filesystem fixture did not compete with history: {history_priority_state:?}"
    );
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-c"])
        .status()
        .unwrap();
    wait_for_zle(&server, &sync_file);

    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0"])
        .arg("zsuggestion-command-with-a-very-long-name v")
        .status()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut long_candidate_capture = String::new();
    while Instant::now() < deadline {
        long_candidate_capture = capture_pane(&server, false);
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
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-c"])
        .status()
        .unwrap();
    wait_for_zle(&server, &sync_file);

    Command::new("tmux")
        .args([
            "-L",
            &server,
            "send-keys",
            "-l",
            "-t",
            "test:0.0",
            "echo before",
        ])
        .status()
        .unwrap();
    thread::sleep(Duration::from_millis(100));
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0"])
        .arg("\u{1b}[200~ pasted\u{1b}[201~")
        .status()
        .unwrap();
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0", " after"])
        .status()
        .unwrap();
    thread::sleep(Duration::from_millis(500));
    dump_zle_state(&server);
    let typed_after_paste = fs::read_to_string(&state_dump).unwrap();
    let typed_after_paste_fields: Vec<_> = typed_after_paste.trim_end().split('|').collect();
    assert_eq!(
        typed_after_paste_fields[3], "echo before pasted after",
        "typing after bracketed paste replaced the existing line"
    );
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-c"])
        .status()
        .unwrap();
    wait_for_zle(&server, &sync_file);

    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0"])
        .arg("zsuggestion-native-fixture na")
        .status()
        .unwrap();
    let immediate_capture = capture_pane(&server, false);
    assert!(
        !immediate_capture.contains("native/path/file"),
        "native completion ran synchronously instead of waiting for the debounce"
    );
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut native_capture = String::new();
    while Instant::now() < deadline {
        native_capture = capture_pane(&server, false);
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
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0", "t"])
        .status()
        .unwrap();
    let retained_capture = capture_pane(&server, false);
    assert!(
        retained_capture.contains('╭')
            && retained_capture.contains("zsuggestion-native-fixture native/path/file"),
        "matching candidates flashed off while the next request was pending:\n{retained_capture}"
    );
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-n", "C-n"])
        .status()
        .unwrap();
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "BTab"])
        .status()
        .unwrap();
    dump_zle_state(&server);
    let shifted_state = fs::read_to_string(&state_dump).unwrap();
    assert!(
        shifted_state.contains("|2|zsuggestion-native-fixture native/second/file"),
        "Shift-Tab did not select the next candidate: {shifted_state:?}"
    );
    let shifted_fields: Vec<_> = shifted_state.trim_end().split('|').collect();
    assert_eq!(
        shifted_fields[15], "ive/second/file",
        "selected candidate was not reflected in ghost text"
    );

    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0", "i"])
        .status()
        .unwrap();
    dump_zle_state(&server);
    let typed_state = fs::read_to_string(&state_dump).unwrap();
    assert!(
        typed_state.contains("|zsuggestion-native-fixture nati|zsuggestion-native-fixture nati|"),
        "ordinary typing did not update the buffer after selection: {typed_state:?}"
    );
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "BSpace"])
        .status()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    while Instant::now() < deadline {
        native_capture = capture_pane(&server, false);
        if native_capture.contains("zsuggestion-native-fixture native/path/file") {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        native_capture.contains("zsuggestion-native-fixture native/path/file"),
        "Backspace did not resume completion from the edited buffer:\n{native_capture}"
    );
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-n"])
        .status()
        .unwrap();
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "Tab"])
        .status()
        .unwrap();
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-x", "C-d"])
        .status()
        .unwrap();
    thread::sleep(Duration::from_millis(20));
    let segment_capture = capture_pane(&server, false);
    let state_after_tab = fs::read_to_string(&state_dump).unwrap();
    assert!(
        state_after_tab.contains(
            "|zsuggestion-native-fixture native/|zsuggestion-native-fixture native/|path/file"
        ) && state_after_tab.contains("|1|zsuggestion-native-fixture native/path/file"),
        "Tab did not accept the next path segment and reset selection; state was {state_after_tab:?}:\n{segment_capture}"
    );
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "Tab"])
        .status()
        .unwrap();
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-x", "C-d"])
        .status()
        .unwrap();
    thread::sleep(Duration::from_millis(20));
    let second_segment_capture = capture_pane(&server, false);
    let state_after_second_tab = fs::read_to_string(&state_dump).unwrap();
    assert!(
        state_after_second_tab.contains(
            "|zsuggestion-native-fixture native/path/|zsuggestion-native-fixture native/path/|file"
        ),
        "a repeated Tab did not accept the next path segment; state was {state_after_second_tab:?}:\n{second_segment_capture}"
    );
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "Tab"])
        .status()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut follow_up_capture = String::new();
    let mut follow_up_state = String::new();
    while Instant::now() < deadline {
        follow_up_capture = capture_pane(&server, false);
        dump_zle_state(&server);
        follow_up_state = fs::read_to_string(&state_dump).unwrap();
        if follow_up_state.contains("next-value") {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        follow_up_state.contains("next-value"),
        "native completion did not continue after accepting a complete match; state was {follow_up_state:?}:\n{follow_up_capture}"
    );

    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-c"])
        .status()
        .unwrap();
    wait_for_zle(&server, &sync_file);
    Command::new("tmux")
        .args([
            "-L",
            &server,
            "send-keys",
            "-l",
            "-t",
            "test:0.0",
            "ssh ali",
        ])
        .status()
        .unwrap();
    wait_for_pane(&server, "ssh alice@example.com");
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "Tab"])
        .status()
        .unwrap();
    dump_zle_state(&server);
    let ssh_user_state = fs::read_to_string(&state_dump).unwrap();
    assert!(
        ssh_user_state.contains("|ssh alice@|ssh alice@|example.com"),
        "Tab did not stop after the SSH user boundary: {ssh_user_state:?}"
    );

    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-c"])
        .status()
        .unwrap();
    wait_for_zle(&server, &sync_file);
    Command::new("tmux")
        .args([
            "-L",
            &server,
            "send-keys",
            "-l",
            "-t",
            "test:0.0",
            "scp zzu",
        ])
        .status()
        .unwrap();
    wait_for_pane(&server, "scp zzuser@example.com:/srv/app/file");
    for expected in [
        "|scp zzuser@|scp zzuser@|example.com:/srv/app/file",
        "|scp zzuser@example.com:/|scp zzuser@example.com:/|srv/app/file",
        "|scp zzuser@example.com:/srv/|scp zzuser@example.com:/srv/|app/file",
    ] {
        Command::new("tmux")
            .args(["-L", &server, "send-keys", "-t", "test:0.0", "Tab"])
            .status()
            .unwrap();
        dump_zle_state(&server);
        let remote_path_state = fs::read_to_string(&state_dump).unwrap();
        assert!(
            remote_path_state.contains(expected),
            "Tab did not stop at remote path boundary {expected:?}: {remote_path_state:?}"
        );
    }

    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-c"])
        .status()
        .unwrap();
    wait_for_zle(&server, &sync_file);
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0"])
        .arg("zsuggestion-native-fixture na")
        .status()
        .unwrap();
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "Tab"])
        .status()
        .unwrap();
    thread::sleep(Duration::from_millis(350));
    let tab_capture = capture_pane(&server, false);
    assert!(
        !tab_capture.contains("completion cannot be used recursively"),
        "ticker entered completion recursively during native Tab:\n{tab_capture}"
    );

    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-c"])
        .status()
        .unwrap();
    wait_for_zle(&server, &sync_file);
    let scp_path_prefix = format!("scp -r {}/filesystem-", temporary.path().display());
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0"])
        .arg(&scp_path_prefix)
        .status()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut scp_capture = String::new();
    while Instant::now() < deadline {
        scp_capture = capture_pane(&server, false);
        if scp_capture.contains("1/2") && scp_capture.matches("File").count() >= 2 {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        scp_capture.contains("1/2") && scp_capture.matches("File").count() >= 2,
        "generic filesystem candidates were absent for scp:\n{scp_capture}"
    );
    dump_zle_state(&server);
    let scp_before_tab = fs::read_to_string(&state_dump).unwrap();
    assert!(
        scp_before_tab.contains(&format!("|{scp_path_prefix}|{scp_path_prefix}|")),
        "zsuggestion did not retain the scp path prefix: {scp_before_tab:?}"
    );
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "Tab"])
        .status()
        .unwrap();
    thread::sleep(Duration::from_millis(50));
    dump_zle_state(&server);
    let scp_after_tab = fs::read_to_string(&state_dump).unwrap();
    assert!(
        scp_after_tab.contains(&format!("|{scp_path_prefix}|{scp_path_prefix}|")),
        "ambiguous path Tab selected an arbitrary file: {scp_after_tab:?}"
    );
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0", "a"])
        .status()
        .unwrap();
    thread::sleep(Duration::from_millis(100));
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "Tab"])
        .status()
        .unwrap();
    dump_zle_state(&server);
    let scp_unique_path = fs::read_to_string(&state_dump).unwrap();
    assert!(
        scp_unique_path.contains(&format!(
            "|{}filesystem-alpha |{}filesystem-alpha |",
            &scp_path_prefix[..scp_path_prefix.len() - "filesystem-".len()],
            &scp_path_prefix[..scp_path_prefix.len() - "filesystem-".len()]
        )),
        "Tab did not accept the unique filesystem candidate: {scp_unique_path:?}"
    );

    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-c"])
        .status()
        .unwrap();
    wait_for_zle(&server, &sync_file);
    let kitty_prefix = format!("nv {}/kitty.con", temporary.path().display());
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0"])
        .arg(&kitty_prefix)
        .status()
        .unwrap();
    wait_for_pane(&server, "f.bak");
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "Tab"])
        .status()
        .unwrap();
    dump_zle_state(&server);
    let common_path_state = fs::read_to_string(&state_dump).unwrap();
    let common_path_fields: Vec<_> = common_path_state.trim_end().split('|').collect();
    assert_eq!(common_path_fields[3], format!("{kitty_prefix}f"));
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "Tab"])
        .status()
        .unwrap();
    dump_zle_state(&server);
    let exact_path_state = fs::read_to_string(&state_dump).unwrap();
    let exact_path_fields: Vec<_> = exact_path_state.trim_end().split('|').collect();
    assert_eq!(exact_path_fields[3], format!("{kitty_prefix}f "));

    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-c"])
        .status()
        .unwrap();
    wait_for_zle(&server, &sync_file);
    let source_command = format!(
        "zle -D _zsuggestion-native-space; source {}",
        shell_quote(zdotdir.join(".zshrc").to_str().unwrap())
    );
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0"])
        .arg(source_command)
        .status()
        .unwrap();
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "Enter"])
        .status()
        .unwrap();
    thread::sleep(Duration::from_millis(300));
    let rebound_file = temporary.path().join("reloaded-bindings");
    let rebound_command = format!(
        "{{ bindkey '^I'; bindkey '^[[Z'; bindkey '^@'; }} > {}",
        shell_quote(rebound_file.to_str().unwrap())
    );
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0"])
        .arg(rebound_command)
        .status()
        .unwrap();
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "Enter"])
        .status()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && !rebound_file.exists() {
        thread::sleep(Duration::from_millis(20));
    }
    let rebound = fs::read_to_string(&rebound_file).unwrap();
    assert!(rebound.contains(r#""^I" zsuggestion-tab"#));
    assert!(rebound.contains("zsuggestion-shift-tab"));
    assert!(rebound.contains(r#""^@" zsuggestion-complete"#));

    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0", "zsu"])
        .status()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut reloaded_capture = String::new();
    while Instant::now() < deadline {
        reloaded_capture = capture_pane(&server, false);
        if reloaded_capture.contains('╭') {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        reloaded_capture.contains('╭'),
        "zsuggestion did not restart its ticker after .zshrc was sourced:\n{reloaded_capture}"
    );

    for command in [
        "echo fuzzy-history-target",
        "echo fuzzy-history-target-secondary",
    ] {
        let status = Command::new(binary)
            .args([
                "record",
                "--command",
                command,
                "--cwd",
                temporary.path().to_str().unwrap(),
                "--exit-code",
                "0",
                "--session",
                "fuzzy-test",
            ])
            .env("ZSUGGESTION_CONFIG", &config)
            .env("ZSUGGESTION_STATE_DIR", &state)
            .env("ZSUGGESTION_SOCKET", &socket)
            .status()
            .unwrap();
        assert!(status.success(), "failed to seed fuzzy history");
    }
    let fuzzy_execute_command = format!(
        "print -r -- fuzzy-enter-executed > {}",
        shell_quote(fuzzy_execute_file.to_str().unwrap())
    );
    let status = Command::new(binary)
        .args([
            "record",
            "--command",
            &fuzzy_execute_command,
            "--cwd",
            temporary.path().to_str().unwrap(),
            "--exit-code",
            "0",
            "--session",
            "fuzzy-execute-test",
        ])
        .env("ZSUGGESTION_CONFIG", &config)
        .env("ZSUGGESTION_STATE_DIR", &state)
        .env("ZSUGGESTION_SOCKET", &socket)
        .status()
        .unwrap();
    assert!(status.success(), "failed to seed fuzzy execution history");
    let ls_history = format!("ls {}", temporary.path().display());
    let status = Command::new(binary)
        .args([
            "record",
            "--command",
            &ls_history,
            "--cwd",
            temporary.path().to_str().unwrap(),
            "--exit-code",
            "0",
            "--session",
            "preview-test",
        ])
        .env("ZSUGGESTION_CONFIG", &config)
        .env("ZSUGGESTION_STATE_DIR", &state)
        .env("ZSUGGESTION_SOCKET", &socket)
        .status()
        .unwrap();
    assert!(status.success(), "failed to seed ls preview history");
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-c"])
        .status()
        .unwrap();
    wait_for_zle(&server, &sync_file);
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0", "  fzht"])
        .status()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut fuzzy_capture = String::new();
    while Instant::now() < deadline {
        fuzzy_capture = capture_pane(&server, false);
        if fuzzy_capture.contains("echo fuzzy-history-target") {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        fuzzy_capture.contains("echo fuzzy-history-target"),
        "inline fuzzy history result was absent:\n{fuzzy_capture}"
    );
    dump_zle_state(&server);
    let fuzzy_state = fs::read_to_string(&state_dump).unwrap();
    let fuzzy_fields: Vec<_> = fuzzy_state.trim_end().split('|').collect();
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
        cursor_x(&server, "test:0.0"),
        6,
        "fuzzy query did not retain ZLE's real cursor"
    );
    assert!(
        fuzzy_fields[1].parse::<usize>().unwrap() >= 2,
        "fuzzy fixture did not produce scrollable candidates: {fuzzy_state:?}"
    );
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-n"])
        .status()
        .unwrap();
    dump_zle_state(&server);
    let scrolled_fuzzy_state = fs::read_to_string(&state_dump).unwrap();
    let scrolled_fuzzy_fields: Vec<_> = scrolled_fuzzy_state.trim_end().split('|').collect();
    assert_ne!(scrolled_fuzzy_fields[6], fuzzy_fields[6]);
    assert_eq!(scrolled_fuzzy_fields[8], "");
    assert_eq!(
        scrolled_fuzzy_fields[15],
        format!("  → {}", scrolled_fuzzy_fields[6]),
        "scrolling did not update fuzzy ghost text"
    );

    fs::write(&escape_request, "").unwrap();
    dump_zle_state(&server);
    let escaped_state = fs::read_to_string(&state_dump).unwrap();
    let escaped_fields: Vec<_> = escaped_state.trim_end().split('|').collect();
    assert_eq!(
        escaped_fields[3], "",
        "fuzzy escape state: {escaped_state:?}"
    );
    assert_eq!(escaped_fields[7], "0");
    assert_eq!(escaped_fields[9], "");

    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0", "  fzht"])
        .status()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    while Instant::now() < deadline {
        fuzzy_capture = capture_pane(&server, false);
        if fuzzy_capture.contains("echo fuzzy-history-target") {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "Tab"])
        .status()
        .unwrap();
    dump_zle_state(&server);
    let accepted_fuzzy_state = fs::read_to_string(&state_dump).unwrap();
    let accepted_fields: Vec<_> = accepted_fuzzy_state.trim_end().split('|').collect();
    assert!(accepted_fields[3].starts_with("echo fuzzy-history-target"));
    assert_eq!(accepted_fields[7], "0");

    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-c"])
        .status()
        .unwrap();
    wait_for_zle(&server, &sync_file);

    Command::new("tmux")
        .args([
            "-L",
            &server,
            "send-keys",
            "-l",
            "-t",
            "test:0.0",
            "  fuzzy-enter-executed",
        ])
        .status()
        .unwrap();
    wait_for_pane(&server, "Enter run");
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "Enter"])
        .status()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    while Instant::now() < deadline && !fuzzy_execute_file.exists() {
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        fs::read_to_string(&fuzzy_execute_file).unwrap(),
        "fuzzy-enter-executed\n",
        "fuzzy Enter selected the command without executing it"
    );
    wait_for_zle(&server, &sync_file);

    Command::new("tmux")
        .args([
            "-L",
            &server,
            "send-keys",
            "-l",
            "-t",
            "test:0.0",
            "echo first  stale",
        ])
        .status()
        .unwrap();
    dump_zle_state(&server);
    let first_interrupted_fuzzy = fs::read_to_string(&state_dump).unwrap();
    let first_interrupted_fields: Vec<_> = first_interrupted_fuzzy.trim_end().split('|').collect();
    assert_eq!(first_interrupted_fields[7], "1");
    assert_eq!(first_interrupted_fields[8], "echo first");
    assert_eq!(first_interrupted_fields[9], "stale");
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-c"])
        .status()
        .unwrap();
    wait_for_zle(&server, &sync_file);

    Command::new("tmux")
        .args([
            "-L",
            &server,
            "send-keys",
            "-l",
            "-t",
            "test:0.0",
            "echo second  fresh",
        ])
        .status()
        .unwrap();
    dump_zle_state(&server);
    let second_fuzzy = fs::read_to_string(&state_dump).unwrap();
    let second_fuzzy_fields: Vec<_> = second_fuzzy.trim_end().split('|').collect();
    assert_eq!(second_fuzzy_fields[7], "1");
    assert_eq!(second_fuzzy_fields[8], "echo second");
    assert_eq!(second_fuzzy_fields[9], "fresh");
    fs::write(&escape_request, "").unwrap();
    dump_zle_state(&server);
    let escaped_second_fuzzy = fs::read_to_string(&state_dump).unwrap();
    let escaped_second_fields: Vec<_> = escaped_second_fuzzy.trim_end().split('|').collect();
    assert_eq!(escaped_second_fields[3], "echo second");
    assert_eq!(escaped_second_fields[7], "0");
    assert_eq!(escaped_second_fields[8], "");
    assert_eq!(escaped_second_fields[9], "");
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-c"])
        .status()
        .unwrap();
    wait_for_zle(&server, &sync_file);
    Command::new("tmux")
        .args([
            "-L",
            &server,
            "resize-window",
            "-t",
            "test:0",
            "-x",
            "140",
            "-y",
            "40",
        ])
        .status()
        .unwrap();
    thread::sleep(Duration::from_millis(50));
    Command::new("tmux")
        .args([
            "-L",
            &server,
            "send-keys",
            "-l",
            "-t",
            "test:0.0",
            "echo fuzzy",
        ])
        .status()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut history_capture = String::new();
    while Instant::now() < deadline {
        history_capture = capture_pane(&server, false);
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
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-c"])
        .status()
        .unwrap();
    wait_for_zle(&server, &sync_file);
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0", "ls "])
        .status()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut command_preview_capture = String::new();
    while Instant::now() < deadline {
        command_preview_capture = capture_pane(&server, true);
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
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-c"])
        .status()
        .unwrap();
    wait_for_zle(&server, &sync_file);
    let preview_prefix = preview_file.to_str().unwrap().strip_suffix("txt").unwrap();
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-l", "-t", "test:0.0"])
        .arg(format!("zsuggestion-preview-fixture {preview_prefix}"))
        .status()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut preview_capture = String::new();
    while Instant::now() < deadline {
        preview_capture = capture_pane(&server, false);
        if preview_capture.contains("preview-first-line") {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    dump_zle_state(&server);
    let preview_state = fs::read_to_string(&state_dump).unwrap();
    assert!(
        preview_capture.contains("preview-first-line") && preview_capture.contains("Preview:"),
        "lazy wide-terminal file preview was absent; state was {preview_state:?}:\n{preview_capture}"
    );
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:0.0", "C-n"])
        .status()
        .unwrap();
    thread::sleep(Duration::from_millis(20));
    let switching_preview = capture_pane(&server, false);
    assert!(
        !switching_preview.contains("preview-first-line"),
        "the previous preview remained visible while switching rows:\n{switching_preview}"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut second_preview = String::new();
    while Instant::now() < deadline {
        second_preview = capture_pane(&server, false);
        if second_preview.contains("preview-other-candidate") {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        second_preview.contains("preview-other-candidate")
            && !second_preview.contains("preview-first-line"),
        "the second preview did not replace the first cleanly:\n{second_preview}"
    );
    Command::new("tmux")
        .args([
            "-L",
            &server,
            "resize-window",
            "-t",
            "test:0",
            "-x",
            "80",
            "-y",
            "24",
        ])
        .status()
        .unwrap();

    let ascii_shell = format!("env LC_ALL=C LANG=C {shell_environment} zsh -l");
    let status = Command::new("tmux")
        .args([
            "-L",
            &server,
            "new-window",
            "-d",
            "-t",
            "test",
            "-n",
            "ascii",
        ])
        .arg(ascii_shell)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "failed to start the non-UTF-8 Zsh fixture"
    );
    wait_for_zle_target(&server, "test:ascii.0", &sync_file);
    Command::new("tmux")
        .args([
            "-L",
            &server,
            "send-keys",
            "-l",
            "-t",
            "test:ascii.0",
            "zsu",
        ])
        .status()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut ascii_capture = String::new();
    while Instant::now() < deadline {
        ascii_capture = capture_target(&server, "test:ascii.0", false);
        if ascii_capture.contains("+-") && ascii_capture.contains("zsuggestion") {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        ascii_capture.contains("+-") && ascii_capture.contains("| > "),
        "ASCII fallback menu was absent:\n{ascii_capture}"
    );
    assert!(
        !ascii_capture.contains("<e2>") && !ascii_capture.contains('╭'),
        "non-UTF-8 menu still contained Unicode rendering:\n{ascii_capture}"
    );
    assert_eq!(cursor_x(&server, "test:ascii.0"), 5);
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:ascii.0", "BSpace"])
        .status()
        .unwrap();
    thread::sleep(Duration::from_millis(50));
    assert_eq!(cursor_x(&server, "test:ascii.0"), 4);
    Command::new("tmux")
        .args([
            "-L",
            &server,
            "send-keys",
            "-t",
            "test:ascii.0",
            "C-x",
            "C-d",
        ])
        .status()
        .unwrap();
    thread::sleep(Duration::from_millis(20));
    let ascii_state = fs::read_to_string(&state_dump).unwrap();
    let ascii_fields: Vec<_> = ascii_state.trim_end().split('|').collect();
    assert_eq!(ascii_fields[3], "zs");
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-l", "-t", "test:ascii.0", "u"])
        .status()
        .unwrap();
    thread::sleep(Duration::from_millis(150));
    Command::new("tmux")
        .args([
            "-L",
            &server,
            "resize-window",
            "-t",
            "test:ascii",
            "-x",
            "20",
            "-y",
            "15",
        ])
        .status()
        .unwrap();
    Command::new("tmux")
        .args(["-L", &server, "send-keys", "-t", "test:ascii.0", "C-l"])
        .status()
        .unwrap();
    thread::sleep(Duration::from_millis(100));
    let narrow_capture = capture_target(&server, "test:ascii.0", false);
    assert!(
        narrow_capture.contains("zsuggestion") && !narrow_capture.contains("+-"),
        "narrow terminal did not retain a compact suggestion:\n{narrow_capture}"
    );
    assert_eq!(cursor_x(&server, "test:ascii.0"), 5);
}

fn dump_zle_state(server: &str) {
    Command::new("tmux")
        .args(["-L", server, "send-keys", "-t", "test:0.0", "C-x", "C-d"])
        .status()
        .unwrap();
    thread::sleep(Duration::from_millis(20));
}

fn wait_for_zle(server: &str, sync_file: &Path) {
    wait_for_zle_target(server, "test:0.0", sync_file);
}

fn wait_for_zle_target(server: &str, target: &str, sync_file: &Path) {
    let _ = fs::remove_file(sync_file);
    let deadline = Instant::now() + Duration::from_secs(4);
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
    panic!(
        "ZLE did not become ready in {target}:\n{}",
        capture_target(server, target, false)
    );
}

fn capture_pane(server: &str, include_escape_sequences: bool) -> String {
    capture_target(server, "test:0.0", include_escape_sequences)
}

fn wait_for_pane(server: &str, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(4);
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

fn capture_target(server: &str, target: &str, include_escape_sequences: bool) -> String {
    let mut command = Command::new("tmux");
    command.args(["-L", server, "capture-pane", "-p"]);
    if include_escape_sequences {
        command.arg("-e");
    }
    let output = command.args(["-t", target]).output().unwrap();
    String::from_utf8(output.stdout).unwrap()
}

fn command_exists(name: &str) -> bool {
    Command::new(name)
        .arg("-V")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

struct ServerGuard {
    server: String,
    binary: std::path::PathBuf,
    config: std::path::PathBuf,
    state: std::path::PathBuf,
    socket: std::path::PathBuf,
    started: bool,
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        if self.started {
            let _ = Command::new("tmux")
                .args(["-L", &self.server, "kill-server"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
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
