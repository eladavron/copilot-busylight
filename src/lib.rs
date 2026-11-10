//! Kuando Busylight status light for the GitHub Copilot agent.

pub mod autostart;
pub mod folder_dialog;
pub mod hook;
mod icons;
mod kuando;
mod light;
pub mod registry;
pub mod sessions;
pub mod settings;
pub mod tray;
mod usb;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const TRAY_EXE: &str = "copilot-busylight.exe";
pub const HOOK_EXE: &str = "copilot-busylight-hook.exe";

/// NUL-terminated UTF-16 for Win32 APIs.
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
