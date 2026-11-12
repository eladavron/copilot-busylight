# copilot-busylight

Turns a Kuando Busylight into a status light for the GitHub Copilot agent in VS Code.

| Light | Meaning |
|---|---|
| Pulsing red | The agent is working (you sent a prompt, it's running tools) |
| Green | The agent finished and is waiting for your next prompt |
| 3 green blinks | A chat finished; then the light shows the state of the remaining chats |
| Yellow | The agent is asking you a question |
| Off | No active chat, or paused from the tray menu |

With several chats open, the most urgent state wins: yellow, then red, then green.

It's two small Windows executables with no runtime to install: a tray app that drives the light, and a hook that Copilot runs on agent events. Tested with a Kuando Busylight Omega; the Alpha uses the same protocol.

> **Disclaimer:** this entire app, including its code, scripts and documentation, was written by an AI coding assistant (GitHub Copilot). Special care was given to attributing every external source: the code it builds on and the libraries it uses are credited in [Credits](#credits), [NOTICE](NOTICE) and [THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt), along with their licenses.

## How it works

```
Copilot agent ──hooks──▶ copilot-busylight-hook.exe ──▶ ~/.copilot/busylight/sessions/<chat>
                                                                       │
                              copilot-busylight.exe (tray) ◀───────────┘
                                                                       │  USB HID, rewritten every second
                                                                       ▼
                                                               Kuando Busylight
```

- **Hook:** Copilot runs `copilot-busylight-hook.exe <Event>` on each agent event (prompt sent, tool use, turn finished and so on). It writes that chat's state to a small file and exits right away.
- **Tray app:** `copilot-busylight.exe` reads those files and drives the light. It has to keep running because the Busylight switches itself off a few seconds after the last USB message. It uses about 12 MB of memory and very little CPU.

## Requirements

- Windows 10 or 11
- VS Code with GitHub Copilot, using the **Copilot** session target. The **Local** session target can be enabled too; see [Local session target](#local-session-target-optional) below.
- A Kuando Busylight (Alpha or Omega)

## Installation

1. Plug in the Busylight.
2. Run `copilot-busylight-setup-<version>.exe` (or build it yourself, see [Building](#building)).
3. When asked whether the Busylight is plugged into this computer, choose **Yes**.
4. Start a **new** chat with the **Copilot** session target and send a prompt. The light should pulse red while the agent works and turn green when it finishes. To use the **Local** session target as well, see [Local session target](#local-session-target-optional) below.

The installer doesn't need administrator rights; it installs for your user only. The exes aren't code-signed, so Windows SmartScreen may warn the first time; choose **More info → Run anyway** if you trust the source.

What the installer does:
- Puts the tray app, the hook and a copy of itself in `~/.copilot/busylight`.
- Writes `busylight.json` to `~/.copilot/hooks` (or `$COPILOT_HOME/hooks` if that's set).
- Writes a WSL version of the hooks into every installed WSL distro (see [WSL](#wsl)).
- Sets the tray app to start with Windows and starts it.
- Adds **Copilot Busylight** to **Settings → Apps → Installed apps**, for uninstalling.

Run a newer setup exe to update; it closes the running tray app first.

Command-line options:

| Option | Effect |
|---|---|
| `--quiet` | No dialogs. Keeps the current mode and folder when updating; a first install is for the PC with the light. |
| `--local` | Install for the PC with the light, without asking. |
| `--remote` | Install for an RDP server, without asking (see below). |
| `--sessions-dir <path>` | Use this sessions folder. On the PC with the light it's otherwise the default (change it later from the tray menu); on a server it skips the folder question. |
| `--uninstall` | Remove everything. Add `--quiet` to skip the confirmation. |

## The tray icon

The icon is a dot in the light's current colour (a grey ring when no chat is active, a grey dot when paused). Hover over it for the status, or click it for the menu:
- **Status lines:** what the light is showing, how many chats are working, and whether the light is connected.
- **Pause light:** turns the light off until you untick it, for example during a meeting.
- **Sessions folder...:** where chat states are stored; `~/.copilot/busylight/sessions` by default. Opens a folder picker at the current folder, then asks you to confirm: the change applies to new chats, and chats that are open now disappear from the light until their next activity.
- **Start with Windows:** turns autostart on or off.
- **Quit:** turns the light off and exits.

Settings are stored under `HKEY_CURRENT_USER\Software\CopilotBusylight`.

New tray icons are often hidden under the **^** arrow on the taskbar. Drag it onto the taskbar to keep it visible.

## Local session target (optional)

The **Local** session target (VS Code's older built-in agent) only runs hooks if these settings are on. Add them to your VS Code `settings.json`:

```json
"chat.useHooks": true,
"chat.hookFilesLocations": {
    ".github/hooks": true,
    ".claude/settings.local.json": true,
    ".claude/settings.json": true,
    "~/.copilot/hooks": true,
    "~/.claude/settings.json": true
}
```

Local chats don't report when they close, so an idle Local chat stops counting after 30 minutes.

If your organization disables Copilot preview features, VS Code forces `chat.useHooks` off and Local chats can't drive the light. The **Copilot** session target is not affected.

## WSL

Chats running inside WSL (for example VS Code connected to WSL, or Copilot CLI in a WSL terminal) drive the light too. Setup writes `~/.copilot/hooks/busylight.json` into each WSL distro it finds.

> **Note:** setup only sets up the distros that exist when it runs. After adding a new WSL distro, run setup again (it keeps your settings):
>
> ```powershell
> & "$HOME\.copilot\busylight\copilot-busylight-setup.exe" --quiet
> ```

The WSL hooks don't run any Windows program. Each event saves Copilot's JSON as a file in `~/.copilot/busylight/inbox` on Windows (reached as `/mnt/c/...`), and the tray app reads and deletes those files. Events that arrive while the tray app isn't running are discarded when it starts.

## Remote server over RDP (optional)

If you run VS Code on a server you reach over Remote Desktop, the hooks run on the server. They can still drive the light on your PC through RDP drive sharing.

1. Install copilot-busylight on your PC (the one with the light), as above.
2. In Remote Desktop Connection, go to **Show Options → Local Resources → More…**, tick **Drives → Local Disk (C:)**, and connect.
3. On the server, run the same setup exe and choose **No** when asked whether the Busylight is plugged into this computer.
4. Confirm your PC's sessions folder, as seen from the server. The suggestion is `\\tsclient\C\Users\<server username>\.copilot\busylight\sessions`. If your username on the PC is different, choose **No** and type the right `\\tsclient\...` path in the folder picker's box.
5. Start a new chat on the server.

The hook on the server writes each chat's state into that folder, which is your PC's own sessions folder reached through RDP drive sharing. Your PC's tray app can't tell server chats from local ones, so the light shows both.

The server also gets a tray app, which doesn't drive a light. Its icon and tooltip show what your light shows, and whether your PC's folder can be reached. Its menu:
- **Pause this server's chats:** stops reporting this server's chats to the light and removes the ones already reported. Chats on other machines still drive the light.
- **Sessions folder...:** change the `\\tsclient` path.
- **Start with Windows** and **Quit**, as on the PC.

When you disconnect, `\\tsclient` disappears: the server tray shows "NOT reachable", and the light keeps the server chats' last state until the timeouts clear it (see below). While connected, your whole C: drive is visible from the server; that's how RDP drive sharing works.

## Timeouts

- A "working" chat with no new events counts as idle after **15 minutes**. Interrupting a turn with Esc sends no "finished" event, so this stops the light staying red. A single agent step that runs longer than that, such as a long build, will show green too early.
- An idle chat stops counting after **30 minutes**.

These and the colours are constants in `src/sessions.rs` and `src/light.rs`; change them and rebuild.

## Troubleshooting

- **Nothing happens in a chat.** Hooks only load when a chat starts, so start a new one. Check that the session target is **Copilot**, and that `busylight.json` appears on the **Hooks** page under Chat → Open Customizations.
- **Check the tray app.** Hover over the tray icon: it shows the current state and whether the light is connected. If the state changes but the light shows "not connected", replug the light; the app retries every 5 seconds.
- **Test without Copilot.** Pipe a fake event into the hook and watch the light and tray icon:

  ```powershell
  '{"session_id":"test"}' | & "$HOME\.copilot\busylight\copilot-busylight-hook.exe" UserPromptSubmit
  '{"session_id":"test"}' | & "$HOME\.copilot\busylight\copilot-busylight-hook.exe" SessionEnd
  ```

- **Light stays red after pressing Esc.** It goes green on your next prompt, or after 15 minutes.
- **Permission prompts don't turn the light yellow.** Copilot only documents that event for Copilot CLI, so in VS Code yellow may only appear when the agent asks a question.
- **Antivirus or SmartScreen warns about the exe.** The exes aren't code-signed. Build them yourself from source if your organisation requires it.

## Building

Rust with the GNU target is enough; Visual Studio isn't needed.

1. Install Rust from [rustup.rs](https://rustup.rs) and choose the `x86_64-pc-windows-gnu` host, or run:

   ```powershell
   rustup-init.exe -y --default-host x86_64-pc-windows-gnu --profile minimal
   ```

   The MSVC target also works if you have the Visual Studio C++ build tools.

2. Build and test:

   ```powershell
   .\build.ps1 -Test      # runs the unit tests, then builds target\release\*.exe
   .\package.ps1          # also copies dist\copilot-busylight-setup-<version>.exe
   ```

The repository is a Cargo workspace: the root crate is the tray app and hook, and `installer/` is the setup exe. The installer embeds the release builds of the other two, so `build.ps1` builds them first; a plain `cargo build` only builds the root crate.

`windows-sys` is pinned to 0.59 because later versions need a full MinGW `dlltool` on the GNU target.

## Uninstall

Remove **Copilot Busylight** from **Settings → Apps → Installed apps**, or run:

```powershell
& "$HOME\.copilot\busylight\copilot-busylight-setup.exe" --uninstall
```

This closes the tray app (turning the light off), and removes the files, the hooks files (including the WSL ones), autostart and the settings. On an RDP server it removes only that server's chat states from your PC's folder.

## Credits

The USB protocol in `src/kuando.rs` is ported from [busylight-core](https://github.com/JnyJny/busylight-core) by Erik O'Shaughnessy (Apache-2.0); the unit tests check the packets against it byte for byte. The design (hooks write state, a background program owns the device) follows [claude-busylight](https://github.com/alexx-avdeev/claude-busylight) by Alexey Avdeev (0BSD).

## License

Copyright 2026 Elad Avron. Licensed under the [Apache License, Version 2.0](LICENSE); see [NOTICE](NOTICE) for attributions.

The binaries also include third-party code: Rust crates (serde_json, windows-sys and others), the Rust standard library and the MinGW-w64 runtime. Their notices are in [THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt). Keep `LICENSE`, `NOTICE` and `THIRD-PARTY-NOTICES.txt` with any copy you redistribute, and update `THIRD-PARTY-NOTICES.txt` when dependencies change.
