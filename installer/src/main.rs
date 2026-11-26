//! `agent-busylight-setup.exe`: installs or uninstalls agent-busylight for the
//! current user. The two program exes are embedded.
//!
//! Flags: `--quiet`, `--local`, `--remote`, `--sessions-dir <path>`, `--copilot`,
//! `--claude`, `--all`, `--uninstall`.

#![windows_subsystem = "windows"]

mod dialogs;
mod files;
mod registry;

use std::path::{Path, PathBuf};
use std::process::Command;

use agent_busylight::harness::{self, Harness};
use agent_busylight::{APP_NAME, HOOK_EXE, TRAY_EXE, VERSION, autostart, folder_dialog, hook, sessions, settings};

use dialogs::{Answer, ask_keep_folder, ask_mode, choose_harnesses, confirm, error, info, warning};

const TRAY_BYTES: &[u8] = include_bytes!("../../target/release/agent-busylight.exe");
const HOOK_BYTES: &[u8] = include_bytes!("../../target/release/agent-busylight-hook.exe");
/// Shipped next to the exes to meet the redistribution terms of the licenses involved.
const LICENSE_FILES: &[(&str, &str)] = &[
    ("LICENSE.txt", include_str!("../../LICENSE")),
    ("NOTICE.txt", include_str!("../../NOTICE")),
    ("THIRD-PARTY-NOTICES.txt", include_str!("../../THIRD-PARTY-NOTICES.txt")),
];

const SETUP_EXE: &str = "agent-busylight-setup.exe";

#[derive(Default)]
struct Options {
    quiet: bool,
    remote: Option<bool>,
    sessions_dir: Option<PathBuf>,
    harnesses: Option<Vec<Harness>>,
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
        let flag = arg.to_ascii_lowercase();
        let flag = flag.trim_start_matches(['-', '/']);
        match flag {
            _ if is_quiet_flag(&arg) => options.quiet = true,
            "remote" => options.remote = Some(true),
            "local" => options.remote = Some(false),
            "uninstall" | "u" => options.uninstall = true,
            "all" => options.harnesses = Some(Harness::ALL.to_vec()),
            "sessions-dir" | "client-state-dir" => {
                let dir = args.next().ok_or_else(|| format!("{arg} needs a folder path"))?;
                options.sessions_dir = Some(PathBuf::from(dir));
            }
            _ => {
                let harness = Harness::ALL
                    .into_iter()
                    .find(|harness| harness.flag() == flag)
                    .ok_or_else(|| format!("Unknown option: {arg}"))?;
                let chosen = options.harnesses.get_or_insert_with(Vec::new);
                if !chosen.contains(&harness) {
                    chosen.push(harness);
                }
            }
        }
    }
    Ok(options)
}

fn main() {
    dialogs::use_visual_styles();
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
    let first_install = !settings::exist();
    // An update from copilot-busylight 0.2.x keeps its mode and folder.
    settings::copy_legacy();
    let Some((remote, sessions_dir, chosen)) = ask(options) else {
        if first_install {
            // Cancelled: leave no settings behind.
            settings::delete_all();
        }
        return Ok(());
    };

    let target = sessions::data_dir();
    files::stop_tray_app()?;
    remove_legacy_install(&chosen);
    let tray = target.join(TRAY_EXE);
    files::write(&tray, TRAY_BYTES)?;
    files::write(&target.join(HOOK_EXE), HOOK_BYTES)?;
    for (name, text) in LICENSE_FILES {
        files::write(&target.join(name), text.as_bytes())?;
    }
    let setup_copy = target.join(SETUP_EXE);
    copy_self_to(&setup_copy)?;
    let _ = std::fs::create_dir_all(hook::inbox_dir());

    let mut wsl_lines = Vec::new();
    let mut failures = Vec::new();
    for harness in Harness::ALL {
        if chosen.contains(&harness) {
            match harness.install() {
                Ok(distros) if !distros.is_empty() => {
                    wsl_lines.push(format!("{}: {}", harness.name(), distros.join(", ")));
                }
                Ok(_) => {}
                Err(message) => failures.push(format!("{}: {message}", harness.name())),
            }
        } else if harness.is_installed()
            && let Err(message) = harness.uninstall()
        {
            failures.push(format!("{}: {message}", harness.name()));
        }
    }

    let saved = settings::set_remote(remote)
        .and_then(|()| settings::set_sessions_dir(&sessions_dir, remote))
        .and_then(|()| settings::set_harnesses(&chosen));
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
        let mut message = finished_message(remote, &sessions_dir, &chosen);
        if !wsl_lines.is_empty() {
            message.push_str(&format!(
                "\n\nAlso installed the hooks in WSL:\n{}",
                wsl_lines.join("\n")
            ));
        }
        if failures.is_empty() {
            info(&message);
        } else {
            message.push_str(&format!(
                "\n\nSome hooks could not be changed:\n{}\n\nFix the problem, then use Hooks in the tray menu.",
                failures.join("\n")
            ));
            warning(&message);
        }
    }
    if !failures.is_empty() && options.quiet {
        return Err(failures.join("\n"));
    }
    Ok(())
}

/// The mode, sessions folder and harnesses; `None` means setup was cancelled.
fn ask(options: &Options) -> Option<(bool, PathBuf, Vec<Harness>)> {
    let remote = match options.remote {
        Some(remote) => remote,
        // An existing install keeps its mode when updated quietly.
        None if options.quiet => settings::is_remote(),
        None => match ask_mode() {
            Answer::Local => false,
            Answer::Remote => true,
            Answer::Cancel => return None,
        },
    };
    let sessions_dir = choose_sessions_dir(options, remote)?;
    let chosen = choose(options)?;
    Some((remote, sessions_dir, chosen))
}

/// The harnesses from the flags, or the ones chosen last time, or the detected ones;
/// asked unless quiet. `None` means setup was cancelled.
fn choose(options: &Options) -> Option<Vec<Harness>> {
    if let Some(chosen) = &options.harnesses {
        return Some(chosen.clone());
    }
    let default = settings::harnesses().unwrap_or_else(|| {
        let detected: Vec<Harness> = Harness::ALL.into_iter().filter(|h| h.is_detected()).collect();
        if detected.is_empty() {
            vec![Harness::Copilot]
        } else {
            detected
        }
    });
    if options.quiet {
        return Some(default);
    }
    choose_harnesses(&default)
}

/// Removes copilot-busylight 0.2.x, which this replaces. Its settings were
/// copied already; its Copilot hooks are replaced when Copilot is chosen.
fn remove_legacy_install(chosen: &[Harness]) {
    let legacy = sessions::legacy_data_dir();
    let legacy_hooks = harness::legacy_copilot_file();
    if !legacy.is_dir() && legacy_hooks.is_none() {
        settings::delete_legacy();
        return;
    }
    if !chosen.contains(&Harness::Copilot) {
        let _ = Harness::Copilot.uninstall();
    }
    if settings::is_remote() {
        // That folder is on the user's PC; remove only this server's chats from it.
        files::remove_prefixed(&settings::legacy_sessions_dir(), &settings::file_prefix(true));
    }
    files::remove_dir(&legacy);
    autostart::remove_legacy();
    registry::delete_legacy_uninstall_entry();
    settings::delete_legacy();
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

fn finished_message(remote: bool, sessions_dir: &Path, chosen: &[Harness]) -> String {
    let next = if chosen.is_empty() {
        "No agent hooks were installed. Add them from the tray menu (Hooks) once an agent is installed.".to_owned()
    } else {
        let steps: Vec<&str> = chosen
            .iter()
            .map(|harness| match harness {
                Harness::Copilot => {
                    "- GitHub Copilot: start a NEW chat in VS Code (session target: Copilot) or Copilot CLI."
                }
                Harness::Claude => "- Claude Code: start a NEW session (CLI, VS Code or desktop app).",
            })
            .collect();
        format!(
            "Chats and sessions that are already open don't load new hooks:\n{}",
            steps.join("\n")
        )
    };
    if !remote {
        return format!("{APP_NAME} {VERSION} is installed and running in the system tray.\n\n{next}");
    }
    let mut message = format!(
        "{APP_NAME} {VERSION} is installed on this server. Its tray icon shows what your light shows.\n\n\
         Chats here are reported to:\n{}\n\n{next}",
        sessions_dir.display()
    );
    if !sessions_dir.is_dir() {
        message.push_str(&format!(
            "\n\nWarning: that folder can't be reached right now. In Remote Desktop Connection, \
             share drive C: (Local Resources > More > Drives), make sure {APP_NAME} {VERSION} is \
             installed on your PC, and reconnect. You can change the folder later from the tray menu."
        ));
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
    if !options.quiet && !confirm(&format!("Remove {APP_NAME} from this user account?")) {
        return Ok(());
    }
    let target = sessions::data_dir();
    let remote = settings::is_remote();
    let sessions_dir = settings::sessions_dir();
    files::stop_tray_app()?;
    autostart::disable();
    registry::delete_uninstall_entry();
    let failures: Vec<String> = Harness::ALL
        .into_iter()
        .filter_map(|harness| harness.uninstall().err().map(|e| format!("{}: {e}", harness.name())))
        .collect();
    remove_legacy_install(&[]);
    files::remove_dir(&hook::inbox_dir());
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
        if failures.is_empty() {
            info(&format!("{APP_NAME} has been removed."));
        } else {
            warning(&format!(
                "{APP_NAME} has been removed, but some hooks could not be:\n{}",
                failures.join("\n")
            ));
        }
    }
    Ok(())
}
