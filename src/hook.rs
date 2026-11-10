//! `copilot-busylight hook <Event>`: records one chat's state from a Copilot
//! hook event (JSON on stdin).

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

use serde_json::Value;

use crate::sessions::Light;
use crate::settings;

const ASK_TOOLS: &[&str] = &["vscode_askQuestions", "ask_user", "AskUserQuestion"];
const ATTENTION_NOTIFICATIONS: &[&str] = &["permission_prompt", "elicitation_dialog"];

#[derive(Debug, PartialEq)]
pub enum Action {
    Set(Light),
    End,
}

pub fn run(event: &str) {
    // Never fail: a failing PreToolUse hook would deny the agent's tool call.
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_ok()
        && let Ok(payload) = serde_json::from_str::<Value>(&input)
        && let Some(action) = classify(event, &payload)
    {
        let remote = settings::is_remote();
        // While paused, ending a chat still clears it so it doesn't linger on the light.
        if !(remote && settings::is_reporting_paused() && action != Action::End) {
            let name = format!("{}{}", settings::file_prefix(remote), file_name(&session_id(&payload)));
            let _ = record(&state_dir(), &name, action);
        }
    }
    print!("{{}}");
}

/// BUSYLIGHT_STATE_DIR overrides the configured folder (for testing and scripts).
fn state_dir() -> PathBuf {
    std::env::var_os("BUSYLIGHT_STATE_DIR")
        .filter(|dir| !dir.is_empty())
        .map_or_else(settings::sessions_dir, PathBuf::from)
}

fn text<'a>(payload: &'a Value, key: &str) -> Option<&'a str> {
    payload.get(key).and_then(Value::as_str)
}

pub fn classify(event: &str, payload: &Value) -> Option<Action> {
    match event {
        "PreToolUse" => {
            let tool = text(payload, "tool_name")
                .or_else(|| text(payload, "toolName"))
                .unwrap_or("");
            let state = if ASK_TOOLS.contains(&tool) {
                Light::Attention
            } else {
                Light::Busy
            };
            Some(Action::Set(state))
        }
        "Notification" => text(payload, "notification_type")
            .filter(|kind| ATTENTION_NOTIFICATIONS.contains(kind))
            .map(|_| Action::Set(Light::Attention)),
        "UserPromptSubmit" | "PostToolUse" | "PostToolUseFailure" | "SubagentStart" | "SubagentStop" | "PreCompact" => {
            Some(Action::Set(Light::Busy))
        }
        "SessionStart" | "Stop" => Some(Action::Set(Light::Idle)),
        "SessionEnd" => Some(Action::End),
        _ => None,
    }
}

fn session_id(payload: &Value) -> String {
    payload
        .get("session_id")
        .or_else(|| payload.get("sessionId"))
        .map(|id| id.as_str().map_or_else(|| id.to_string(), str::to_owned))
        .unwrap_or_else(|| "default".to_owned())
}

/// FNV-1a, so an arbitrary session id can never escape the sessions folder.
pub fn file_name(session_id: &str) -> String {
    let hash = session_id.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

fn record(dir: &Path, name: &str, action: Action) -> io::Result<()> {
    let path = dir.join(name);
    let Action::Set(state) = action else {
        return match fs::remove_file(&path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
    };
    fs::create_dir_all(dir)?;
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&tmp, state.as_str())?;
    // Another hook for the same chat may be replacing the file at this moment.
    let mut result = fs::rename(&tmp, &path);
    for _ in 0..3 {
        if result.is_ok() {
            break;
        }
        sleep(Duration::from_millis(20));
        result = fs::rename(&tmp, &path);
    }
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classifies_events() {
        let empty = json!({});
        assert_eq!(classify("UserPromptSubmit", &empty), Some(Action::Set(Light::Busy)));
        assert_eq!(classify("Stop", &empty), Some(Action::Set(Light::Idle)));
        assert_eq!(classify("SessionEnd", &empty), Some(Action::End));
        assert_eq!(classify("Unknown", &empty), None);
        assert_eq!(
            classify("PreToolUse", &json!({"tool_name": "vscode_askQuestions"})),
            Some(Action::Set(Light::Attention))
        );
        assert_eq!(
            classify("PreToolUse", &json!({"toolName": "edit"})),
            Some(Action::Set(Light::Busy))
        );
        assert_eq!(
            classify("Notification", &json!({"notification_type": "permission_prompt"})),
            Some(Action::Set(Light::Attention))
        );
        assert_eq!(
            classify("Notification", &json!({"notification_type": "shell_completed"})),
            None
        );
    }

    #[test]
    fn session_ids() {
        assert_eq!(session_id(&json!({"session_id": "abc"})), "abc");
        assert_eq!(session_id(&json!({"sessionId": 42})), "42");
        assert_eq!(session_id(&json!({})), "default");
    }

    #[test]
    fn file_names_are_safe_and_stable() {
        let name = file_name(r"..\..\evil");
        assert_eq!(name.len(), 16);
        assert!(name.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(name, file_name(r"..\..\evil"));
        assert_ne!(name, file_name("other"));
    }

    #[test]
    fn records_and_ends_a_session() {
        let dir = std::env::temp_dir().join(format!("copilot-busylight-test-{}", std::process::id()));
        let name = file_name("s1");
        record(&dir, &name, Action::Set(Light::Busy)).unwrap();
        let path = dir.join(&name);
        assert_eq!(fs::read_to_string(&path).unwrap(), "busy");
        record(&dir, &name, Action::Set(Light::Idle)).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "idle");
        record(&dir, &name, Action::End).unwrap();
        assert!(!path.exists());
        record(&dir, &name, Action::End).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }
}
