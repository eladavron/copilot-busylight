//! `copilot-busylight-setup.exe`: installs or uninstalls copilot-busylight for
//! the current user. The two program exes and the hooks file are embedded.
//!
//! Flags: `--quiet`, `--local`, `--remote`, `--sessions-dir <path>`, `--uninstall`.

#![windows_subsystem = "windows"]

mod dialogs;
mod files;
mod registry;

use std::path::{Path, PathBuf};
use std::process::Command;

use copilot_busylight::{HOOK_EXE, TRAY_EXE, VERSION, autostart, folder_dialog, sessions, settings};

use dialogs::{Answer, ask_keep_folder, ask_mode, confirm, error, info};

const TRAY_BYTES: &[u8] = include_bytes!("../../target/release/copilot-busylight.exe");
const HOOK_BYTES: &[u8] = include_bytes!("../../target/release/copilot-busylight-hook.exe");
const HOOKS_JSON: &str = include_str!("../../hooks/busylight.json");
/// Shipped next to the exes to meet the redistribution terms of the licenses involved.
const LICENSE_FILES: &[(&str, &str)] = &[
    ("LICENSE.txt", include_str!("../../LICENSE")),
    ("NOTICE.txt", include_str!("../../NOTICE")),
    ("THIRD-PARTY-NOTICES.txt", include_str!("../../THIRD-PARTY-NOTICES.txt")),
];

const SETUP_EXE: &str = "copilot-busylight-setup.exe";

#[derive(Default)]
struct Options {
    quiet: bool,
    remote: Option<bool>,
    sessions_dir: Option<PathBuf>,
    uninstall: bool,
}

fn is_quiet_flag(arg: &str) -> bool {
    matches!(
        arg.to_ascii_lowercase().trim_start_matches(['-', '/']),
        "quiet" | "q" | "silent" | "s"
    )
}

fn parse_args() -> Result<Options, String> {
    let mut options = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_ascii_lowercase().trim_start_matches(['-', '/']) {
            _ if is_quiet_flag(&arg) => options.quiet = true,
            "remote" => options.remote = Some(true),
            "local" => options.remote = Some(false),
            "uninstall" | "u" => options.uninstall = true,
            "sessions-dir" | "client-state-dir" => {
                let dir = args.next().ok_or_else(|| format!("{arg} needs a folder path"))?;
                options.sessions_dir = Some(PathBuf::from(dir));
            }
            _ => return Err(format!("Unknown option: {arg}")),
        }
    }
    Ok(options)
}

fn main() {
    let options = match parse_args() {
        Ok(options) => options,
        Err(message) => {
            if !std::env::args().any(|arg| is_quiet_flag(&arg)) {
                error(&message);
            }
            std::process::exit(2);
        }
    };
    let result = if options.uninstall {
        uninstall(&options)
    } else {
        install(&options)
    };
    if let Err(message) = result {
        if !options.quiet {
            error(&message);
        }
        std::process::exit(1);
    }
}

fn install(options: &Options) -> Result<(), String> {
    let remote = match options.remote {
        Some(remote) => remote,
        // An existing install keeps its mode when updated quietly.
        None if options.quiet => settings::is_remote(),
        None => match ask_mode() {
            Answer::Local => false,
            Answer::Remote => true,
            Answer::Cancel => return Ok(()),
        },
    };
    let Some(sessions_dir) = choose_sessions_dir(options, remote) else {
        return Ok(());
    };

    let target = sessions::data_dir();
    files::stop_tray_app()?;
    let tray = target.join(TRAY_EXE);
    files::write(&tray, TRAY_BYTES)?;
    files::write(&target.join(HOOK_EXE), HOOK_BYTES)?;
    files::write(&files::hooks_dir().join("busylight.json"), HOOKS_JSON.as_bytes())?;
    for (name, text) in LICENSE_FILES {
        files::write(&target.join(name), text.as_bytes())?;
    }
    let setup_copy = target.join(SETUP_EXE);
    copy_self_to(&setup_copy)?;

    let saved = settings::set_remote(remote).and_then(|()| settings::set_sessions_dir(&sessions_dir, remote));
    saved.map_err(|code| format!("Could not save the settings (error {code})."))?;
    if !remote {
        // Only a remote server can pause its reporting; a local install starts unpaused.
        let _ = settings::set_reporting_paused(false);
        let _ = std::fs::create_dir_all(&sessions_dir);
    }
    registry::write_uninstall_entry(&setup_copy, &target, remote)
        .map_err(|code| format!("Could not register the uninstaller (error {code})."))?;

    if !autostart::enable_for(&tray) {
        return Err("Could not set the tray app to start with Windows.".to_owned());
    }
    Command::new(&tray)
        .current_dir(&target)
        .spawn()
        .map_err(|e| format!("Installed, but could not start the tray app: {e}"))?;

    if !options.quiet {
        info(&finished_message(remote, &sessions_dir));
    }
    Ok(())
}

/// The folder from `--sessions-dir`, the existing setting, or the default; the
/// user can change it on a server unless quiet. `None` means setup was cancelled.
fn choose_sessions_dir(options: &Options, remote: bool) -> Option<PathBuf> {
    if let Some(dir) = &options.sessions_dir {
        return Some(dir.clone());
    }
    // Keeps a folder chosen earlier in the tray menu when updating.
    let mut dir = if settings::is_remote() == remote {
        settings::sessions_dir()
    } else {
        settings::default_sessions_dir(remote)
    };
    if options.quiet || !remote {
        return Some(dir);
    }
    loop {
        if ask_keep_folder(&dir)? {
            return Some(dir);
        }
        let title = "Select a shared folder the PC with the Busylight can also reach (the same folder as the one set on the PC). You can type a path.";
        if let Some(chosen) = folder_dialog::browse(std::ptr::null_mut(), title, &dir) {
            dir = chosen;
        }
    }
}

fn finished_message(remote: bool, sessions_dir: &Path) -> String {
    let next =
        "Start a NEW chat in VS Code (session target: Copilot) - chats that are already open don't load new hooks.";
    if !remote {
        return format!("Copilot Busylight {VERSION} is installed and running in the system tray.\n\n{next}");
    }
    let mut message = format!(
        "Copilot Busylight {VERSION} is installed on this server. Its tray icon shows what your light shows.\n\n\
         Chats here are reported to:\n{}\n\n{next}",
        sessions_dir.display()
    );
    if !sessions_dir.is_dir() {
        message.push_str(
            "\n\nWarning: that folder can't be reached right now. In Remote Desktop Connection, \
             share drive C: (Local Resources > More > Drives), make sure Copilot Busylight is \
             installed on your PC, and reconnect. You can change the folder later from the tray menu.",
        );
    }
    message
}

fn copy_self_to(destination: &Path) -> Result<(), String> {
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    if files::same_file(&current, destination) {
        return Ok(());
    }
    let bytes = std::fs::read(&current).map_err(|e| format!("Could not read the installer: {e}"))?;
    files::write(destination, &bytes)
}

fn uninstall(options: &Options) -> Result<(), String> {
    if !options.quiet && !confirm("Remove Copilot Busylight from this user account?") {
        return Ok(());
    }
    let target = sessions::data_dir();
    let remote = settings::is_remote();
    let sessions_dir = settings::sessions_dir();
    files::stop_tray_app()?;
    autostart::disable();
    registry::delete_uninstall_entry();
    files::remove(&files::hooks_dir().join("busylight.json"));
    files::remove(&target.join(TRAY_EXE));
    files::remove(&target.join(HOOK_EXE));
    for (name, _) in LICENSE_FILES {
        files::remove(&target.join(name));
    }
    if remote {
        // The folder is on the user's PC; remove only this server's chats from it.
        files::remove_prefixed(&sessions_dir, &settings::file_prefix(true));
    } else {
        files::remove_dir(&sessions_dir);
    }
    settings::delete_all();

    let setup_copy = target.join(SETUP_EXE);
    let running_from_install = std::env::current_exe().is_ok_and(|exe| files::same_file(&exe, &setup_copy));
    if running_from_install {
        // A running exe can't delete itself; cmd removes it once this process exits.
        files::delete_after_exit(&setup_copy, &target);
    } else {
        files::remove(&setup_copy);
        files::remove_dir(&target);
    }
    if !options.quiet {
        info("Copilot Busylight has been removed.");
    }
    Ok(())
}
