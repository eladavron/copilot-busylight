//! Per-user settings shared by the tray app, the hook and the installer.

use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::WIN32_ERROR;

use crate::harness::Harness;
use crate::{hook, registry, sessions};

const KEY: &str = r"Software\AgentBusylight";
/// Settings of copilot-busylight 0.2.x and earlier.
const LEGACY_KEY: &str = r"Software\CopilotBusylight";

/// Whether this computer has been set up (every install stores the mode).
pub fn exist() -> bool {
    registry::get_dword(KEY, "Remote").is_some()
}

/// Remote: an RDP server that reports its chats to the PC with the light.
pub fn is_remote() -> bool {
    registry::get_dword(KEY, "Remote") == Some(1)
}

pub fn set_remote(remote: bool) -> Result<(), WIN32_ERROR> {
    registry::set_dword(KEY, "Remote", u32::from(remote))
}

pub fn default_sessions_dir(remote: bool) -> PathBuf {
    if remote {
        let user = std::env::var("USERNAME").unwrap_or_default();
        PathBuf::from(format!(r"\\tsclient\C\Users\{user}\.agent-busylight\sessions"))
    } else {
        sessions::data_dir().join("sessions")
    }
}

/// The harnesses chosen at the last install; `None` before the first one.
pub fn harnesses() -> Option<Vec<Harness>> {
    let text = registry::get_string(KEY, "Harnesses")?;
    Some(
        Harness::ALL
            .into_iter()
            .filter(|harness| text.split(',').any(|flag| flag == harness.flag()))
            .collect(),
    )
}

pub fn set_harnesses(harnesses: &[Harness]) -> Result<(), WIN32_ERROR> {
    let text: Vec<&str> = harnesses.iter().map(|harness| harness.flag()).collect();
    registry::set_string(KEY, "Harnesses", &text.join(","))
}

/// The sessions folder copilot-busylight 0.2.x used.
pub fn legacy_sessions_dir() -> PathBuf {
    let remote = registry::get_dword(LEGACY_KEY, "Remote") == Some(1);
    registry::get_string(LEGACY_KEY, "SessionsDir")
        .filter(|dir| !dir.is_empty())
        .map_or_else(|| legacy_default_sessions_dir(remote), PathBuf::from)
}

fn legacy_default_sessions_dir(remote: bool) -> PathBuf {
    if remote {
        let user = std::env::var("USERNAME").unwrap_or_default();
        PathBuf::from(format!(r"\\tsclient\C\Users\{user}\.copilot\busylight\sessions"))
    } else {
        sessions::legacy_data_dir().join("sessions")
    }
}

/// Copies the settings of copilot-busylight 0.2.x here, unless there are settings
/// already. A folder that was the old default becomes the new default.
pub fn copy_legacy() {
    let Some(remote) = registry::get_dword(LEGACY_KEY, "Remote").map(|value| value == 1) else {
        return;
    };
    if exist() {
        return;
    }
    let _ = set_remote(remote);
    let _ = set_reporting_paused(registry::get_dword(LEGACY_KEY, "Paused") == Some(1));
    let custom = registry::get_string(LEGACY_KEY, "SessionsDir")
        .filter(|dir| !dir.is_empty() && !same_path(Path::new(dir), &legacy_default_sessions_dir(remote)));
    if let Some(dir) = custom {
        let _ = set_sessions_dir(&renamed_legacy_dir(&dir), remote);
    }
}

/// A `...\.copilot\busylight\sessions` folder becomes `...\.agent-busylight\sessions`,
/// for example a `\\tsclient` path with a different username typed in.
fn renamed_legacy_dir(dir: &str) -> PathBuf {
    const OLD: &str = r"\.copilot\busylight\sessions";
    let trimmed = dir.trim_end_matches('\\');
    match trimmed.len().checked_sub(OLD.len()) {
        Some(at) if trimmed.is_char_boundary(at) && trimmed[at..].eq_ignore_ascii_case(OLD) => {
            PathBuf::from(format!(r"{}\.agent-busylight\sessions", &trimmed[..at]))
        }
        _ => PathBuf::from(dir),
    }
}

pub fn delete_legacy() {
    registry::delete_key(LEGACY_KEY);
}

pub fn sessions_dir() -> PathBuf {
    registry::get_string(KEY, "SessionsDir")
        .filter(|dir| !dir.is_empty())
        .map_or_else(|| default_sessions_dir(is_remote()), PathBuf::from)
}

/// Stores only a non-default folder, so the default keeps following the username.
pub fn set_sessions_dir(dir: &Path, remote: bool) -> Result<(), WIN32_ERROR> {
    if same_path(dir, &default_sessions_dir(remote)) {
        registry::delete_value(KEY, "SessionsDir");
        Ok(())
    } else {
        registry::set_string(KEY, "SessionsDir", &dir.to_string_lossy())
    }
}

pub fn same_path(a: &Path, b: &Path) -> bool {
    let normalize = |path: &Path| path.to_string_lossy().trim_end_matches('\\').to_lowercase();
    normalize(a) == normalize(b)
}

/// Remote only: the hook stops reporting this server's chats.
pub fn is_reporting_paused() -> bool {
    registry::get_dword(KEY, "Paused") == Some(1)
}

pub fn set_reporting_paused(paused: bool) -> Result<(), WIN32_ERROR> {
    registry::set_dword(KEY, "Paused", u32::from(paused))
}

/// Remote session files start with a tag for this computer, so it can find its own.
pub fn file_prefix(remote: bool) -> String {
    if !remote {
        return String::new();
    }
    let computer = std::env::var("COMPUTERNAME").unwrap_or_default();
    format!("{}-", &hook::file_name(&computer)[..8])
}

pub fn delete_all() {
    registry::delete_key(KEY);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_compare_case_and_slash_insensitively() {
        assert!(same_path(Path::new(r"C:\Users\Me\X\"), Path::new(r"c:\users\me\x")));
        assert!(!same_path(Path::new(r"C:\a"), Path::new(r"C:\b")));
    }

    #[test]
    fn legacy_folders_move_to_the_new_name() {
        assert_eq!(
            renamed_legacy_dir(r"\\tsclient\C\Users\Other\.Copilot\Busylight\Sessions\"),
            PathBuf::from(r"\\tsclient\C\Users\Other\.agent-busylight\sessions")
        );
        assert_eq!(
            renamed_legacy_dir(r"D:\shared\lights"),
            PathBuf::from(r"D:\shared\lights")
        );
    }

    #[test]
    fn remote_prefix_is_a_short_hex_tag() {
        assert_eq!(file_prefix(false), "");
        let prefix = file_prefix(true);
        assert_eq!(prefix.len(), 9);
        assert!(prefix.ends_with('-'));
        assert!(prefix[..8].chars().all(|c| c.is_ascii_hexdigit()));
    }
}
