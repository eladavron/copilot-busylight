//! Claude Code hooks. Claude reads them from its own `settings.json`, which also
//! holds the user's other settings, so ours are merged in and removed again
//! without touching anything else.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

/// Every handler we add mentions this (the hook exe or the inbox folder), and
/// nothing else is expected to, so it identifies ours when removing them.
const MARKER: &str = "agent-busylight";
/// Without a matcher a group runs for every occurrence of the event.
const EVENTS: &[(&str, Option<&str>)] = &[
    ("SessionStart", None),
    ("UserPromptSubmit", None),
    ("PreToolUse", None),
    ("PermissionRequest", None),
    ("PostToolUse", None),
    ("PostToolUseFailure", None),
    ("PreCompact", None),
    ("Notification", Some("permission_prompt|elicitation_dialog|idle_prompt")),
    ("Stop", None),
    ("StopFailure", None),
    ("SessionEnd", None),
];
const BACKUP_SUFFIX: &str = "agent-busylight-backup";

/// `CLAUDE_CONFIG_DIR` replaces `~/.claude` when set.
pub fn config_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|dir| !dir.is_empty())
        .map_or_else(|| crate::home().join(".claude"), PathBuf::from)
}

pub fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

/// Handlers that run the Windows hook exe. Exec form (`args`) spawns it directly,
/// so it works the same whether Claude would use Git Bash or PowerShell.
pub fn windows_handler(hook_exe: &Path) -> impl Fn(&str) -> Value + use<> {
    let exe = hook_exe.to_string_lossy().replace('\\', "/");
    move |event| json!({ "type": "command", "command": exe, "args": [event], "timeout": 5 })
}

/// Handlers for Claude inside WSL, which leave each event in the inbox folder.
pub fn wsl_handler(inbox: &str) -> impl Fn(&str) -> Value + use<> {
    let inbox = inbox.to_owned();
    move |event| json!({ "type": "command", "command": crate::wsl::inbox_command(&inbox, event), "timeout": 5 })
}

fn is_ours(handler: &Value) -> bool {
    let mentions = |value: Option<&Value>| value.is_some_and(|value| value.to_string().contains(MARKER));
    mentions(handler.get("command")) || mentions(handler.get("args"))
}

fn as_object<'a>(value: &'a mut Value, what: &str) -> Result<&'a mut Map<String, Value>, String> {
    value
        .as_object_mut()
        .ok_or_else(|| format!("{what} isn't a JSON object"))
}

/// Removes our handlers, and any group or event they leave empty; true if anything changed.
pub fn remove(settings: &mut Value) -> Result<bool, String> {
    let settings = as_object(settings, "The settings file")?;
    let Some(hooks) = settings.get_mut("hooks") else {
        return Ok(false);
    };
    let hooks = as_object(hooks, "\"hooks\"")?;
    let mut changed = false;
    for groups in hooks.values_mut() {
        let Some(groups) = groups.as_array_mut() else {
            continue;
        };
        let before = groups.len();
        groups.retain_mut(|group| {
            let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            let count = handlers.len();
            handlers.retain(|handler| !is_ours(handler));
            if handlers.len() == count {
                return true;
            }
            changed = true;
            !handlers.is_empty()
        });
        changed |= groups.len() != before;
    }
    if changed {
        hooks.retain(|_, groups| groups.as_array().is_none_or(|groups| !groups.is_empty()));
        if hooks.is_empty() {
            settings.remove("hooks");
        }
    }
    Ok(changed)
}

/// Replaces our handlers with fresh ones made by `handler(event)`.
pub fn add(settings: &mut Value, handler: impl Fn(&str) -> Value) -> Result<(), String> {
    remove(settings)?;
    let settings = as_object(settings, "The settings file")?;
    let hooks = settings.entry("hooks").or_insert_with(|| json!({}));
    let hooks = as_object(hooks, "\"hooks\"")?;
    for &(event, matcher) in EVENTS {
        let groups = hooks.entry(event).or_insert_with(|| json!([]));
        let groups = groups
            .as_array_mut()
            .ok_or_else(|| format!("\"hooks\".\"{event}\" isn't a JSON array"))?;
        let mut group = json!({ "hooks": [handler(event)] });
        if let Some(matcher) = matcher {
            group["matcher"] = json!(matcher);
        }
        groups.push(group);
    }
    Ok(())
}

pub fn contains_ours(settings: &Value) -> bool {
    let Some(hooks) = settings.get("hooks").and_then(Value::as_object) else {
        return false;
    };
    hooks
        .values()
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|group| group.get("hooks").and_then(Value::as_array))
        .flatten()
        .any(is_ours)
}

/// Parses settings text; missing or blank means no settings yet.
pub fn parse(text: Option<&str>) -> Result<Value, String> {
    let text = text.unwrap_or("").trim_start_matches('\u{feff}');
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(text).map_err(|e| format!("it isn't valid JSON ({e})"))
}

/// Applies `edit` to the settings text; `None` if nothing changed.
pub fn edit_text(
    text: Option<&str>,
    edit: impl FnOnce(&mut Value) -> Result<(), String>,
) -> Result<Option<String>, String> {
    let original = parse(text)?;
    let mut settings = original.clone();
    edit(&mut settings)?;
    if settings == original {
        return Ok(None);
    }
    let mut out = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    out.push('\n');
    Ok(Some(out))
}

/// Edits the settings file in place, keeping a copy of the previous version.
/// A symlinked file (dotfiles) is edited at its target and stays a symlink.
pub fn edit_file(path: &Path, edit: impl FnOnce(&mut Value) -> Result<(), String>) -> Result<(), String> {
    let describe = |e: &dyn std::fmt::Display| format!("Could not update {}: {e}", path.display());
    let resolved = fs::canonicalize(path).ok();
    let path = resolved.as_deref().unwrap_or(path);
    let text = match fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(describe(&e)),
    };
    let Some(updated) = edit_text(text.as_deref(), edit).map_err(|e| describe(&e))? else {
        return Ok(());
    };
    if let Some(text) = &text {
        fs::write(backup_path(path), text).map_err(|e| describe(&e))?;
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| describe(&e))?;
    }
    let tmp = path.with_extension("json.agent-busylight-tmp");
    fs::write(&tmp, updated).map_err(|e| describe(&e))?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        describe(&e)
    })
}

pub fn backup_path(settings: &Path) -> PathBuf {
    let mut name = settings.file_name().unwrap_or_default().to_owned();
    name.push(format!(".{BACKUP_SUFFIX}"));
    settings.with_file_name(name)
}

/// Whether the settings file has our hooks; an unreadable file counts as no.
pub fn is_installed(path: &Path) -> bool {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| parse(Some(&text)).ok())
        .is_some_and(|settings| contains_ours(&settings))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handler(event: &str) -> Value {
        windows_handler(Path::new(r"C:\Users\Me\.agent-busylight\agent-busylight-hook.exe"))(event)
    }

    fn user_settings() -> Value {
        json!({
            "model": "opus",
            "hooks": {
                "PreToolUse": [
                    { "matcher": "Bash", "hooks": [{ "type": "command", "command": "check.sh" }] }
                ]
            },
            "permissions": { "allow": ["Bash(ls)"] }
        })
    }

    #[test]
    fn handlers_run_the_hook_exe_directly() {
        assert_eq!(
            handler("Stop"),
            json!({
                "type": "command",
                "command": "C:/Users/Me/.agent-busylight/agent-busylight-hook.exe",
                "args": ["Stop"],
                "timeout": 5
            })
        );
    }

    #[test]
    fn adds_hooks_next_to_the_users_own() {
        let mut settings = user_settings();
        add(&mut settings, handler).unwrap();
        assert!(contains_ours(&settings));
        let pre_tool = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre_tool.len(), 2);
        assert_eq!(pre_tool[0]["matcher"], "Bash");
        assert_eq!(pre_tool[1]["hooks"][0]["args"], json!(["PreToolUse"]));
        assert!(pre_tool[1].get("matcher").is_none());
        assert_eq!(
            settings["hooks"]["Notification"][0]["matcher"],
            "permission_prompt|elicitation_dialog|idle_prompt"
        );
        assert_eq!(settings["hooks"].as_object().unwrap().len(), EVENTS.len());
        assert_eq!(settings["model"], "opus");
        // Keeps the user's key order.
        let keys: Vec<_> = settings.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["model", "hooks", "permissions"]);
    }

    #[test]
    fn adding_twice_does_not_duplicate() {
        let mut once = user_settings();
        add(&mut once, handler).unwrap();
        let mut twice = once.clone();
        add(&mut twice, handler).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn removing_restores_the_users_settings() {
        let mut settings = user_settings();
        add(&mut settings, handler).unwrap();
        assert!(remove(&mut settings).unwrap());
        assert_eq!(settings, user_settings());
        assert!(!remove(&mut settings).unwrap());
    }

    #[test]
    fn removing_the_only_hooks_drops_the_hooks_key() {
        let mut settings = json!({ "model": "opus" });
        add(&mut settings, wsl_handler("/mnt/c/Users/Me/.agent-busylight/inbox")).unwrap();
        assert!(contains_ours(&settings));
        remove(&mut settings).unwrap();
        assert_eq!(settings, json!({ "model": "opus" }));
    }

    #[test]
    fn keeps_other_handlers_in_a_shared_group() {
        let mut settings = json!({ "hooks": { "Stop": [{ "hooks": [
            { "type": "command", "command": "notify.sh" },
            handler("Stop")
        ] }] } });
        remove(&mut settings).unwrap();
        assert_eq!(
            settings,
            json!({ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "notify.sh" }] }] } })
        );
    }

    #[test]
    fn edit_text_handles_missing_blank_and_invalid_files() {
        let added = edit_text(None, |s| add(s, handler)).unwrap().unwrap();
        assert!(contains_ours(&parse(Some(&added)).unwrap()));
        assert!(edit_text(Some("\u{feff}  \n"), |s| add(s, handler)).unwrap().is_some());
        assert_eq!(edit_text(None, |s| remove(s).map(drop)).unwrap(), None);
        assert!(edit_text(Some("{ // comment\n}"), |s| add(s, handler)).is_err());
        assert!(edit_text(Some("[]"), |s| add(s, handler)).is_err());
        assert!(edit_text(Some(r#"{"hooks": []}"#), |s| add(s, handler)).is_err());
    }

    #[test]
    fn edits_the_file_and_keeps_a_backup() {
        let dir = std::env::temp_dir().join(format!("agent-busylight-claude-{}", std::process::id()));
        let path = dir.join("settings.json");
        edit_file(&path, |s| add(s, handler)).unwrap();
        assert!(is_installed(&path));
        assert!(!backup_path(&path).exists());

        fs::write(&path, r#"{"model": "opus"}"#).unwrap();
        edit_file(&path, |s| add(s, handler)).unwrap();
        assert_eq!(fs::read_to_string(backup_path(&path)).unwrap(), r#"{"model": "opus"}"#);
        edit_file(&path, |s| remove(s).map(drop)).unwrap();
        assert!(!is_installed(&path));
        assert_eq!(
            parse(Some(&fs::read_to_string(&path).unwrap())).unwrap(),
            json!({"model": "opus"})
        );

        fs::write(&path, "not json").unwrap();
        assert!(edit_file(&path, |s| add(s, handler)).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "not json");
        fs::remove_dir_all(dir).unwrap();
    }
}
