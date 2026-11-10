//! The Apps & features entry.

use std::path::Path;

use copilot_busylight::registry::{delete_key, set_dword, set_string};
use copilot_busylight::{TRAY_EXE, VERSION};
use windows_sys::Win32::Foundation::WIN32_ERROR;

const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\CopilotBusylight";

pub fn write_uninstall_entry(setup_exe: &Path, install_dir: &Path, remote: bool) -> Result<(), WIN32_ERROR> {
    let exe = setup_exe.display();
    let name = if remote {
        "Copilot Busylight (remote server)"
    } else {
        "Copilot Busylight"
    };
    set_string(UNINSTALL_KEY, "DisplayName", name)?;
    set_string(UNINSTALL_KEY, "DisplayVersion", VERSION)?;
    set_string(UNINSTALL_KEY, "Publisher", "copilot-busylight")?;
    set_string(UNINSTALL_KEY, "InstallLocation", &install_dir.display().to_string())?;
    set_string(
        UNINSTALL_KEY,
        "DisplayIcon",
        &install_dir.join(TRAY_EXE).display().to_string(),
    )?;
    set_string(UNINSTALL_KEY, "UninstallString", &format!("\"{exe}\" --uninstall"))?;
    set_string(
        UNINSTALL_KEY,
        "QuietUninstallString",
        &format!("\"{exe}\" --uninstall --quiet"),
    )?;
    set_dword(UNINSTALL_KEY, "NoModify", 1)?;
    set_dword(UNINSTALL_KEY, "NoRepair", 1)
}

pub fn delete_uninstall_entry() {
    delete_key(UNINSTALL_KEY);
}
