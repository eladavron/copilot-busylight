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
    if io::stdin().read_to_string(&mut input).is_ok() {
        record_event(event, &input);
    }
    print!("{{}}");
}

/// Records one hook event (`input` is the hook's JSON payload).
pub fn record_event(event: &str, input: &str) {
    let Ok(payload) = serde_json::from_str::<Value>(input) else {
        return;
    };
    let Some(action) = classify(event, &payload) else {
        return;
    };
    let remote = settings::is_remote();
    // While paused, ending a chat still clears it so it doesn't linger on the light.
    if !(remote && settings::is_reporting_paused() && action != Action::End) {
        let name = format!("{}{}", settings::file_prefix(remote), file_name(&session_id(&payload)));
        let _ = record(&state_dir(), &name, action);
    }
}

/// WSL hooks can't run the Windows hook, so they leave each payload here as
/// `<nanoseconds>.<pid>.<Event>` (written as `<nanoseconds>.<pid>.tmp`, then renamed).
pub fn inbox_dir() -> PathBuf {
    crate::sessions::data_dir().join("inbox")
}

/// Records the inbox events in order and deletes them; `apply: false` only deletes,
/// for the backlog left while the tray app wasn't running.
pub fn drain_inbox(dir: &Path, apply: bool) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    names.sort();
    for name in names {
        let event = name.rsplit_once('.').map_or("", |(_, event)| event);
        if apply && event == "tmp" {
            continue;
        }
        let path = dir.join(&name);
        if apply && let Ok(input) = fs::read_to_string(&path) {
            record_event(event, &input);
        }
        let _ = fs::remove_file(&path);
    }
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

    #[test]
    fn drains_the_inbox_in_order() {
        let base = std::env::temp_dir().join(format!("copilot-busylight-inbox-{}", std::process::id()));
        let (inbox, state) = (base.join("inbox"), base.join("state"));
        fs::create_dir_all(&inbox).unwrap();
        // SAFETY: no other test reads this variable.
        unsafe { std::env::set_var("BUSYLIGHT_STATE_DIR", &state) };
        let payload = r#"{"session_id":"wsl"}"#;
        fs::write(inbox.join("100.1.UserPromptSubmit"), payload).unwrap();
        fs::write(inbox.join("200.1.Stop"), payload).unwrap();
        fs::write(inbox.join("300.2.tmp"), payload).unwrap();
        drain_inbox(&inbox, true);
        let remote = settings::is_remote();
        let session = state.join(format!("{}{}", settings::file_prefix(remote), file_name("wsl")));
        assert_eq!(fs::read_to_string(&session).unwrap(), "idle");
        assert!(inbox.join("300.2.tmp").exists());

        fs::write(inbox.join("400.1.UserPromptSubmit"), payload).unwrap();
        drain_inbox(&inbox, false);
        assert_eq!(fs::read_dir(&inbox).unwrap().count(), 0);
        assert_eq!(fs::read_to_string(&session).unwrap(), "idle");
        unsafe { std::env::remove_var("BUSYLIGHT_STATE_DIR") };
        fs::remove_dir_all(base).unwrap();
    }
}
