//! Hooks for Copilot running inside WSL distros. They can't run the Windows
//! hook, so they leave each event in the inbox folder for the tray app.

use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const HOOKS_JSON: &str = include_str!("../../hooks/busylight-wsl.json");
const INSTALL: &str = r#"mkdir -p "$HOME/.copilot/hooks" && cat > "$HOME/.copilot/hooks/busylight.json""#;
const UNINSTALL: &str = r#"rm -f "$HOME/.copilot/hooks/busylight.json""#;

fn wsl() -> Command {
    let mut command = Command::new("wsl.exe");
    command.creation_flags(CREATE_NO_WINDOW).stderr(Stdio::null());
    command
}

/// Installed distros, except Docker Desktop's internal ones.
fn distros() -> Vec<String> {
    let Ok(output) = wsl().args(["--list", "--quiet"]).stdin(Stdio::null()).output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    // wsl.exe prints its own messages in UTF-16, unless WSL_UTF8=1 is set.
    let text = if output.stdout.contains(&0) {
        let units: Vec<u16> = output
            .stdout
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    text.lines()
        .map(|line| {
            line.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
                .to_owned()
        })
        .filter(|name| !name.is_empty() && !name.starts_with("docker-desktop"))
        .collect()
}

fn shell(distro: &str, script: &str, input: &[u8]) -> bool {
    let child = wsl()
        .args(["-d", distro, "-e", "sh", "-c", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return false;
    };
    let written = child
        .stdin
        .take()
        .is_some_and(|mut stdin| stdin.write_all(input).is_ok());
    child.wait().is_ok_and(|status| status.success()) && written
}

/// The Windows folder as the distro sees it, if it is safe to put in the hook command.
fn linux_path(distro: &str, dir: &Path) -> Option<String> {
    let output = wsl()
        .args(["-d", distro, "-e", "wslpath", "-u"])
        .arg(dir)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let path = String::from_utf8(output.stdout).ok()?.trim_end().to_owned();
    let safe = path.starts_with('/') && !path.contains(['\'', '"', '\\']) && !path.contains(char::is_control);
    (output.status.success() && safe).then_some(path)
}

/// Returns the distros where the hooks were installed.
pub fn install(inbox: &Path) -> Vec<String> {
    distros()
        .into_iter()
        .filter(|distro| {
            linux_path(distro, inbox)
                .is_some_and(|dir| shell(distro, INSTALL, HOOKS_JSON.replace("@INBOX@", &dir).as_bytes()))
        })
        .collect()
}

pub fn uninstall() {
    for distro in distros() {
        shell(&distro, UNINSTALL, b"");
    }
}
