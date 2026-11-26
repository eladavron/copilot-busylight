//! GitHub Copilot hooks: a hooks file of our own in Copilot's hooks folder,
//! read by the VS Code agent (Copilot session target) and Copilot CLI.

use std::path::PathBuf;

use serde_json::{Map, Value, json};

pub const FILE_NAME: &str = "agent-busylight.json";
/// Hooks file name used by copilot-busylight 0.2.x and earlier.
pub const LEGACY_FILE_NAME: &str = "busylight.json";
/// Copilot's event name, then the name the hook exe expects.
const EVENTS: &[(&str, &str)] = &[
    ("sessionStart", "SessionStart"),
    ("userPromptSubmitted", "UserPromptSubmit"),
    ("preToolUse", "PreToolUse"),
    ("postToolUse", "PostToolUse"),
    ("postToolUseFailure", "PostToolUseFailure"),
    ("preCompact", "PreCompact"),
    ("agentStop", "Stop"),
    ("sessionEnd", "SessionEnd"),
    ("notification", "Notification"),
];

/// `COPILOT_HOME` replaces `~/.copilot` when set.
pub fn config_dir() -> PathBuf {
    std::env::var_os("COPILOT_HOME")
        .filter(|dir| !dir.is_empty())
        .map_or_else(|| crate::home().join(".copilot"), PathBuf::from)
}

pub fn hooks_dir() -> PathBuf {
    config_dir().join("hooks")
}

/// Copilot runs hook commands with PowerShell on Windows.
pub fn windows_command(event: &str) -> String {
    format!("& \"$HOME\\.agent-busylight\\{}\" {event}", crate::HOOK_EXE)
}

/// The hooks file, with `command(event)` as each event's command.
pub fn hooks_json(command: impl Fn(&str) -> String) -> String {
    let hooks: Map<String, Value> = EVENTS
        .iter()
        .map(|&(key, event)| {
            let handler = json!({ "type": "command", "command": command(event), "timeoutSec": 5 });
            (key.to_owned(), json!([handler]))
        })
        .collect();
    let mut text = serde_json::to_string_pretty(&json!({ "version": 1, "hooks": hooks })).unwrap_or_default();
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hooks_file_runs_the_hook_exe_for_each_event() {
        let hooks: Value = serde_json::from_str(&hooks_json(windows_command)).unwrap();
        assert_eq!(hooks["version"], 1);
        assert_eq!(hooks["hooks"].as_object().unwrap().len(), EVENTS.len());
        assert_eq!(
            hooks["hooks"]["agentStop"],
            json!([{
                "type": "command",
                "command": r#"& "$HOME\.agent-busylight\agent-busylight-hook.exe" Stop"#,
                "timeoutSec": 5
            }])
        );
        // Copilot's own order, which is easier to read than alphabetical.
        let first = hooks["hooks"].as_object().unwrap().keys().next().cloned();
        assert_eq!(first.as_deref(), Some("sessionStart"));
    }
}
