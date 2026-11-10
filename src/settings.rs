//! Per-user settings shared by the tray app, the hook and the installer.

use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::WIN32_ERROR;

use crate::{hook, registry, sessions};

const KEY: &str = r"Software\CopilotBusylight";

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
        PathBuf::from(format!(r"\\tsclient\C\Users\{user}\.copilot\busylight\sessions"))
    } else {
        sessions::data_dir().join("sessions")
    }
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
    fn remote_prefix_is_a_short_hex_tag() {
        assert_eq!(file_prefix(false), "");
        let prefix = file_prefix(true);
        assert_eq!(prefix.len(), 9);
        assert!(prefix.ends_with('-'));
        assert!(prefix[..8].chars().all(|c| c.is_ascii_hexdigit()));
    }
}
