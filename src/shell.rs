//! Shell integration scripts emitted by `zsuggestion init`.
//!
//! The Zsh frontend lives in `shell/zsuggestion.zsh` as a plain script so it
//! can be edited, highlighted, and syntax-checked (`zsh -n`) on its own. It
//! contains `__ZSUGGESTION_*__` placeholders that are substituted with values
//! from the validated [`Settings`] when the integration is rendered.

use anyhow::Result;

use crate::config::{Settings, completion_key_sequence};

/// The unrendered Zsh integration template.
pub const ZSH_TEMPLATE: &str = include_str!("shell/zsuggestion.zsh");

/// Renders the Zsh integration with every placeholder substituted.
pub fn zsh_integration(settings: &Settings) -> Result<String> {
    let completion = &settings.completion;
    let ui = &settings.ui;
    let flag = |enabled: bool| if enabled { "1" } else { "0" }.to_owned();

    let substitutions = [
        (
            "__ZSUGGESTION_COMPLETION_KEY__",
            completion_key_sequence(&completion.key)?,
        ),
        (
            "__ZSUGGESTION_COMPLETION_KEY_LABEL__",
            completion_key_label(&completion.key),
        ),
        ("__ZSUGGESTION_UI_MENU_WIDTH__", ui.menu_width.to_string()),
        ("__ZSUGGESTION_UI_MAX_VISIBLE__", ui.max_visible.to_string()),
        (
            "__ZSUGGESTION_COMPLETION_MAX_CANDIDATES__",
            completion.max_candidates.to_string(),
        ),
        // Native Zsh results (subcommands, flags) are scrollable like an IDE
        // popup, so they get more room than the conservative daemon tiers.
        (
            "__ZSUGGESTION_NATIVE_LIMIT__",
            completion.max_candidates.max(64).to_string(),
        ),
        ("__ZSUGGESTION_AUTO_COMPINIT__", flag(completion.compinit)),
        ("__ZSUGGESTION_UI_MEASURE_PROMPT__", flag(ui.measure_prompt)),
        (
            "__ZSUGGESTION_UI_PROMPT_OFFSET__",
            ui.prompt_offset.to_string(),
        ),
        ("__ZSUGGESTION_UI_ACCENT__", ui.accent.clone()),
        ("__ZSUGGESTION_UI_TEXT__", ui.text.clone()),
        ("__ZSUGGESTION_UI_MUTED__", ui.muted.clone()),
        ("__ZSUGGESTION_UI_GHOST__", ui.ghost.clone()),
        (
            "__ZSUGGESTION_UI_SELECTED_BACKGROUND__",
            ui.selected_background.clone(),
        ),
        ("__ZSUGGESTION_UI_SELECTED_TEXT__", ui.selected_text.clone()),
        (
            "__ZSUGGESTION_UI_SELECTED_SOURCE__",
            ui.selected_source.clone(),
        ),
    ];

    let mut script = ZSH_TEMPLATE.to_owned();
    for (placeholder, value) in substitutions {
        script = script.replace(placeholder, &value);
    }
    Ok(script)
}

fn completion_key_label(key: &str) -> String {
    match key {
        "ctrl-space" => "Ctrl-Space".to_owned(),
        "shift-tab" => "Shift-Tab".to_owned(),
        "tab" => "Tab".to_owned(),
        key => format!(
            "Ctrl-{}",
            key.trim_start_matches("ctrl-").to_ascii_uppercase()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_every_placeholder() {
        let integration = zsh_integration(&Settings::default()).unwrap();
        assert!(!integration.contains("__ZSUGGESTION_"));
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
        assert!(
            integration
                .contains("builtin compadd -A __zsuggestion_hits -D __zsuggestion_hit_displays")
        );
        assert!(integration.contains("add-zsh-hook precmd _zsuggestion_ensure_compsys"));
        assert!(integration.contains("(( 1 )) || return 0"));
        assert!(integration.contains("zmodload zsh/zle"));
        assert!(integration.contains("bindkey '^[[C' zsuggestion-accept-ghost"));
        assert!(integration.contains("bindkey '^[f' zsuggestion-accept-ghost-word"));
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

        settings.completion.key = "ctrl-x".to_owned();
        settings.ui.selected_background = "#3a3228".to_owned();
        let integration = zsh_integration(&settings).unwrap();
        assert!(integration.contains("bindkey '^X' zsuggestion-complete"));
        assert!(integration.contains("bg=#3a3228"));

        settings.completion.compinit = false;
        settings.ui.measure_prompt = false;
        let integration = zsh_integration(&settings).unwrap();
        assert!(integration.contains("(( 0 )) || return 0"));
        assert!(integration.contains("(( 0 && _ZSUGGESTION_PROMPT_QUERY ))"));
    }

    #[test]
    fn rendered_integration_is_valid_zsh() {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let Ok(mut child) = Command::new("zsh")
            .args(["-n", "-f"])
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        else {
            eprintln!("skipping zsh syntax check because zsh is unavailable");
            return;
        };
        let script = zsh_integration(&Settings::default()).unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(script.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "zsh -n rejected the integration:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn labels_completion_keys() {
        assert_eq!(completion_key_label("ctrl-space"), "Ctrl-Space");
        assert_eq!(completion_key_label("ctrl-x"), "Ctrl-X");
    }
}
