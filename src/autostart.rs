//! "Start with Windows" via the per-user Run registry key.

use std::path::Path;

use crate::registry;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = crate::APP_NAME;
/// Used by copilot-busylight 0.2.x and earlier.
const LEGACY_VALUE_NAME: &str = "Copilot Busylight";

pub fn is_enabled() -> bool {
    registry::get_string(RUN_KEY, VALUE_NAME).is_some()
}

pub fn set_enabled(enabled: bool) -> bool {
    if !enabled {
        return disable();
    }
    std::env::current_exe().is_ok_and(|exe| enable_for(&exe))
}

pub fn disable() -> bool {
    registry::delete_value(RUN_KEY, VALUE_NAME)
}

pub fn enable_for(exe: &Path) -> bool {
    registry::set_string(RUN_KEY, VALUE_NAME, &format!("\"{}\"", exe.display())).is_ok()
}

pub fn remove_legacy() {
    registry::delete_value(RUN_KEY, LEGACY_VALUE_NAME);
}
