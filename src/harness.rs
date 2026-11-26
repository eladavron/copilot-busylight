//! The coding agents ("harnesses") whose hooks drive the light: finding out
//! which are installed, and adding or removing their hooks.

use std::fs;
use std::path::{Path, PathBuf};

use crate::{HOOK_EXE, claude, copilot, home, hook, sessions, wsl};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Harness {
    Copilot,
    Claude,
}

impl Harness {
    pub const ALL: [Harness; 2] = [Harness::Copilot, Harness::Claude];

    pub fn name(self) -> &'static str {
        match self {
            Harness::Copilot => "GitHub Copilot",
            Harness::Claude => "Claude Code",
        }
    }

    /// Its setup command-line flag, without the dashes.
    pub fn flag(self) -> &'static str {
        match self {
            Harness::Copilot => "copilot",
            Harness::Claude => "claude",
        }
    }

    /// Whether the agent seems to be installed on Windows. VS Code counts for
    /// Copilot, which is built into it.
    pub fn is_detected(self) -> bool {
        match self {
            Harness::Copilot => {
                copilot::config_dir().is_dir()
                    || on_path("copilot")
                    || on_path("code")
                    || vscode_dirs().any(|d| d.is_dir())
            }
            Harness::Claude => {
                claude::config_dir().is_dir()
                    || on_path("claude")
                    || home().join(r".local\bin\claude.exe").is_file()
                    || has_vscode_extension("anthropic.claude-code-")
            }
        }
    }

    /// Whether its Windows hooks are in place (WSL isn't checked: it's slow).
    pub fn is_installed(self) -> bool {
        match self {
            Harness::Copilot => copilot_file().is_file(),
            Harness::Claude => claude::is_installed(&claude::settings_path()),
        }
    }

    /// Adds the Windows hooks, then those in WSL; returns the distros where that worked.
    pub fn install(self) -> Result<Vec<String>, String> {
        match self {
            Harness::Copilot => {
                write_atomic(&copilot_file(), &copilot::hooks_json(copilot::windows_command))?;
                remove_legacy_copilot_file();
            }
            Harness::Claude => {
                let handler = claude::windows_handler(&sessions::data_dir().join(HOOK_EXE));
                claude::edit_file(&claude::settings_path(), |settings| claude::add(settings, handler))?;
            }
        }
        let inbox = hook::inbox_dir();
        let _ = fs::create_dir_all(&inbox);
        Ok(wsl::install(self, &inbox))
    }

    /// Removes the Windows and WSL hooks. Claude's are removed from WSL even if
    /// its Windows settings file can't be edited.
    pub fn uninstall(self) -> Result<(), String> {
        let result = match self {
            Harness::Copilot => {
                let _ = fs::remove_file(copilot_file());
                remove_legacy_copilot_file();
                Ok(())
            }
            Harness::Claude => {
                claude::edit_file(&claude::settings_path(), |settings| claude::remove(settings).map(drop))
            }
        };
        wsl::uninstall(self);
        result
    }
}

fn copilot_file() -> PathBuf {
    copilot::hooks_dir().join(copilot::FILE_NAME)
}

/// The hooks file of copilot-busylight 0.2.x, if it is still there and really ours.
pub fn legacy_copilot_file() -> Option<PathBuf> {
    let path = copilot::hooks_dir().join(copilot::LEGACY_FILE_NAME);
    fs::read_to_string(&path)
        .is_ok_and(|text| text.contains("copilot-busylight-hook"))
        .then_some(path)
}

fn remove_legacy_copilot_file() {
    if let Some(path) = legacy_copilot_file() {
        let _ = fs::remove_file(path);
    }
}

fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let describe = |e: std::io::Error| format!("Could not write {}: {e}", path.display());
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(describe)?;
    }
    let tmp = path.with_extension("json.new");
    fs::write(&tmp, text).map_err(describe)?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        describe(e)
    })
}

fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        ["exe", "cmd"]
            .iter()
            .any(|extension| dir.join(program).with_extension(extension).is_file())
    })
}

fn vscode_dirs() -> impl Iterator<Item = PathBuf> {
    let home = home();
    [".vscode", ".vscode-insiders"]
        .into_iter()
        .map(move |name| home.join(name))
}

fn has_vscode_extension(prefix: &str) -> bool {
    vscode_dirs().any(|dir| {
        fs::read_dir(dir.join("extensions")).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with(prefix)
            })
        })
    })
}
