//! Hooks for agents running inside WSL distros. They can't run the Windows
//! hook, so they leave each event in the inbox folder for the tray app.

use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::harness::Harness;
use crate::{claude, copilot};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const COPILOT_FILE: &str = "$HOME/.copilot/hooks/agent-busylight.json";
const LEGACY_COPILOT_FILE: &str = "$HOME/.copilot/hooks/busylight.json";
const CLAUDE_SETTINGS: &str = "$HOME/.claude/settings.json";

fn wsl() -> Command {
    let mut command = Command::new("wsl.exe");
    command.creation_flags(CREATE_NO_WINDOW).stderr(Stdio::null());
    command
}

/// Saves the hook's JSON in the inbox as `<nanoseconds>.<pid>.<event>`, written
/// as `.tmp` first so the tray app never reads half a file.
pub fn inbox_command(inbox: &str, event: &str) -> String {
    format!(
        "{{ d='{inbox}'; f=\"$d/$(date +%s%N).$$\"; cat > \"$f.tmp\" && mv \"$f.tmp\" \"$f.{event}\"; }} 2>/dev/null; echo '{{}}'"
    )
}

/// Installed distros, except Docker Desktop's internal ones.
pub fn distros() -> Vec<String> {
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

/// Runs `script` in the distro with `input` on stdin; returns its stdout if it succeeded.
fn shell(distro: &str, script: &str, input: &[u8]) -> Option<Vec<u8>> {
    let mut child = wsl()
        .args(["-d", distro, "-e", "sh", "-c", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;
    let written = child
        .stdin
        .take()
        .is_some_and(|mut stdin| stdin.write_all(input).is_ok());
    let output = child.wait_with_output().ok()?;
    (output.status.success() && written).then_some(output.stdout)
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

fn write_file(distro: &str, file: &str, text: &str) -> bool {
    let script = format!(r#"mkdir -p "$(dirname "{file}")" && cat > "{file}.new" && mv "{file}.new" "{file}""#);
    shell(distro, &script, text.as_bytes()).is_some()
}

/// The file's text, `Some(None)` if it doesn't exist, `None` if it couldn't be read.
fn read_file(distro: &str, file: &str) -> Option<Option<String>> {
    let script = format!(r#"if [ -e "{file}" ]; then printf x; cat "{file}"; fi"#);
    let output = String::from_utf8(shell(distro, &script, b"")?).ok()?;
    Some(output.strip_prefix('x').map(str::to_owned))
}

fn edit_claude(distro: &str, edit: impl FnOnce(&mut serde_json::Value) -> Result<(), String>) -> bool {
    let Some(text) = read_file(distro, CLAUDE_SETTINGS) else {
        return false;
    };
    match claude::edit_text(text.as_deref(), edit) {
        Ok(None) => true,
        Ok(Some(updated)) => {
            if let Some(text) = &text {
                write_file(distro, &format!("{CLAUDE_SETTINGS}.agent-busylight-backup"), text);
            }
            write_file(distro, CLAUDE_SETTINGS, &updated)
        }
        Err(_) => false,
    }
}

fn install_in(distro: &str, harness: Harness, inbox: &str) -> bool {
    match harness {
        Harness::Copilot => {
            shell(distro, &format!(r#"rm -f "{LEGACY_COPILOT_FILE}""#), b"");
            let text = copilot::hooks_json(|event| inbox_command(inbox, event));
            write_file(distro, COPILOT_FILE, &text)
        }
        Harness::Claude => {
            // Only where Claude Code is installed (it creates ~/.claude on first run).
            shell(distro, r#"[ -d "$HOME/.claude" ]"#, b"").is_some()
                && edit_claude(distro, |settings| claude::add(settings, claude::wsl_handler(inbox)))
        }
    }
}

fn uninstall_in(distro: &str, harness: Harness) {
    match harness {
        Harness::Copilot => {
            shell(
                distro,
                &format!(r#"rm -f "{COPILOT_FILE}" "{LEGACY_COPILOT_FILE}""#),
                b"",
            );
        }
        Harness::Claude => {
            edit_claude(distro, |settings| claude::remove(settings).map(drop));
        }
    }
}

/// Installs the hooks for `harness` in every distro; returns the distros where it worked.
pub fn install(harness: Harness, inbox: &Path) -> Vec<String> {
    distros()
        .into_iter()
        .filter(|distro| linux_path(distro, inbox).is_some_and(|dir| install_in(distro, harness, &dir)))
        .collect()
}

pub fn uninstall(harness: Harness) {
    for distro in distros() {
        uninstall_in(&distro, harness);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbox_command_saves_the_payload_under_the_event_name() {
        assert_eq!(
            inbox_command("/mnt/c/Users/Me/.agent-busylight/inbox", "Stop"),
            r#"{ d='/mnt/c/Users/Me/.agent-busylight/inbox'; f="$d/$(date +%s%N).$$"; cat > "$f.tmp" && mv "$f.tmp" "$f.Stop"; } 2>/dev/null; echo '{}'"#
        );
    }
}
