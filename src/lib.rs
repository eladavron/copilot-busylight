//! Kuando Busylight status light for AI coding agents (GitHub Copilot, Claude Code).

pub mod autostart;
pub mod claude;
pub mod copilot;
pub mod folder_dialog;
pub mod harness;
pub mod hook;
mod icons;
mod kuando;
mod light;
pub mod registry;
pub mod sessions;
pub mod settings;
pub mod tray;
mod usb;
pub mod wsl;

use std::path::PathBuf;

pub const APP_NAME: &str = "Agent Busylight";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const TRAY_EXE: &str = "agent-busylight.exe";
pub const HOOK_EXE: &str = "agent-busylight-hook.exe";

/// NUL-terminated UTF-16 for Win32 APIs.
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The user's home folder.
pub fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
}
