//! File operations and stopping the tray app.

use std::fs;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

use agent_busylight::{APP_NAME, wide};
use windows_sys::Win32::Foundation::{CloseHandle, LPARAM, WPARAM};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId, PostMessageW, WM_CLOSE};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Window class of copilot-busylight 0.2.x and earlier.
const LEGACY_TRAY_WINDOW_CLASS: &str = "CopilotBusylightTray";
const STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// Writes atomically; retries briefly because a just-closed exe can stay locked.
pub fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let describe = |e: std::io::Error| format!("Could not write {}: {e}", path.display());
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(describe)?;
    }
    let tmp = path.with_extension("new");
    fs::write(&tmp, bytes).map_err(describe)?;
    let deadline = Instant::now() + STOP_TIMEOUT;
    loop {
        match fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(_) if Instant::now() < deadline => sleep(Duration::from_millis(200)),
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                return Err(describe(e));
            }
        }
    }
}

pub fn remove(path: &Path) {
    let _ = fs::remove_file(path);
}

pub fn remove_dir(path: &Path) {
    let _ = fs::remove_dir_all(path);
}

pub fn remove_prefixed(dir: &Path, prefix: &str) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(prefix) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

pub fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Asks the tray app, and copilot-busylight's if it is still running, to quit
/// (they turn the light off first) and waits for them.
pub fn stop_tray_app() -> Result<(), String> {
    stop_window(agent_busylight::tray::WINDOW_CLASS)?;
    stop_window(LEGACY_TRAY_WINDOW_CLASS)
}

fn stop_window(class: &str) -> Result<(), String> {
    let class = wide(class);
    let hwnd = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
    if hwnd.is_null() {
        return Ok(());
    }
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        PostMessageW(hwnd, WM_CLOSE, 0 as WPARAM, 0 as LPARAM);
        if process.is_null() {
            sleep(Duration::from_secs(1));
            return Ok(());
        }
        let exited = WaitForSingleObject(process, STOP_TIMEOUT.as_millis() as u32) == 0;
        CloseHandle(process);
        if exited {
            Ok(())
        } else {
            Err(format!(
                "The running {APP_NAME} tray app didn't close. Quit it from its tray menu and try again."
            ))
        }
    }
}

/// Deletes `exe` and then `dir` (if empty) a few seconds after this process exits.
pub fn delete_after_exit(exe: &Path, dir: &Path) {
    let script = format!(
        "ping -n 3 127.0.0.1 >nul & del /f /q \"{}\" & rmdir \"{}\"",
        exe.display(),
        dir.display()
    );
    let _ = Command::new("cmd")
        .arg("/c")
        .raw_arg(&script)
        .creation_flags(CREATE_NO_WINDOW)
        .current_dir(std::env::temp_dir())
        .spawn();
}
