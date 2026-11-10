//! The tray icon, its menu, and the hidden window that receives their messages.

use std::cell::RefCell;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::thread::JoinHandle;

use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW,
    GetCursorPos, GetMessageW, IDOK, MB_ICONWARNING, MB_OKCANCEL, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING,
    MessageBoxW, PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SetForegroundWindow,
    TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_APP, WM_CLOSE, WM_DESTROY,
    WM_ENDSESSION, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSW,
};

use crate::icons::Icons;
use crate::light::{self, Report, Shared};
use crate::sessions::Light;
use crate::{autostart, folder_dialog, settings, wide};

const WM_TRAY: u32 = WM_APP + 1;
const WM_REPORT: u32 = WM_APP + 2;
const TRAY_ID: u32 = 1;
const CMD_PAUSE: i32 = 1;
const CMD_AUTOSTART: i32 = 2;
const CMD_QUIT: i32 = 3;
const CMD_FOLDER: i32 = 4;

struct App {
    hwnd: HWND,
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
    icons: Icons,
    taskbar_created: u32,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Runs `f` on the app unless it is already borrowed (re-entrant message).
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|app| app.try_borrow_mut().ok()?.as_mut().map(f))
}

pub fn run() {
    unsafe {
        let mutex_name = wide("Local\\copilot-busylight");
        // Held until the process exits.
        let _mutex = CreateMutexW(null(), 1, mutex_name.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let instance = GetModuleHandleW(null());
        let class = wide("CopilotBusylightTray");
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..zeroed()
        };
        RegisterClassW(&window_class);
        // A normal (never shown) window, so it receives the TaskbarCreated broadcast.
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            return;
        }

        let remote = settings::is_remote();
        // Remote pause is stored, because the hook (a separate process) must obey it too.
        let paused = remote && settings::is_reporting_paused();
        let shared = Arc::new(Shared::new(remote, settings::sessions_dir(), paused));
        let worker_shared = Arc::clone(&shared);
        let hwnd_value = hwnd as usize;
        let worker = std::thread::spawn(move || {
            light::run(&worker_shared, || {
                PostMessageW(hwnd_value as HWND, WM_REPORT, 0, 0);
            });
        });

        let app = App {
            hwnd,
            shared,
            worker: Some(worker),
            icons: Icons::new(),
            taskbar_created: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
        };
        APP.with(|slot| *slot.borrow_mut() = Some(app));
        with_app(App::add_icon);

        let mut message = zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        APP.with(|slot| slot.borrow_mut().take());
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_TRAY => {
            let event = (lparam & 0xFFFF) as u32;
            if event == WM_RBUTTONUP || event == WM_LBUTTONUP {
                show_menu();
            }
            0
        }
        WM_REPORT => {
            with_app(App::refresh_icon);
            0
        }
        WM_ENDSESSION if wparam != 0 => {
            with_app(App::shutdown);
            0
        }
        // Sent by the setup exe when updating or uninstalling.
        WM_CLOSE => {
            with_app(App::shutdown);
            unsafe { DestroyWindow(hwnd) };
            0
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            0
        }
        _ if with_app(|app| app.taskbar_created == message).unwrap_or(false) => {
            // Explorer restarted; the old icon is gone.
            with_app(App::add_icon);
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

/// The menu loop re-enters the window procedure, so no borrow is held across it.
fn show_menu() {
    let Some((hwnd, report, remote)) = with_app(|app| (app.hwnd, app.shared.report(), app.shared.remote)) else {
        return;
    };
    let command = unsafe {
        let menu = CreatePopupMenu();
        let (status, device) = describe(&report, remote);
        let add = |flags, id: i32, text: &str| {
            let text = wide(text);
            AppendMenuW(menu, flags, id as usize, text.as_ptr());
        };
        add(MF_STRING | MF_GRAYED, 0, &status);
        add(MF_STRING | MF_GRAYED, 0, &device);
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        let pause_label = if remote {
            "Pause this server's chats"
        } else {
            "Pause light"
        };
        add(
            MF_STRING | if report.paused { MF_CHECKED } else { 0 },
            CMD_PAUSE,
            pause_label,
        );
        add(MF_STRING, CMD_FOLDER, "Sessions folder...");
        add(
            MF_STRING | if autostart::is_enabled() { MF_CHECKED } else { 0 },
            CMD_AUTOSTART,
            "Start with Windows",
        );
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        add(MF_STRING, CMD_QUIT, "Quit");

        let mut cursor = POINT { x: 0, y: 0 };
        GetCursorPos(&mut cursor);
        // Without this the menu doesn't close when you click elsewhere.
        SetForegroundWindow(hwnd);
        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            cursor.x,
            cursor.y,
            0,
            hwnd,
            null(),
        );
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        command
    };
    with_app(|app| app.run_command(command));
}

fn confirm_folder_change(hwnd: HWND, dir: &std::path::Path) -> bool {
    let text = wide(&format!(
        "Use this sessions folder?\n\n{}\n\nThe change only applies to new chats. Chats that are open \
         now disappear from the light until their next activity.",
        dir.display()
    ));
    let title = wide("Copilot Busylight");
    unsafe { MessageBoxW(hwnd, text.as_ptr(), title.as_ptr(), MB_OKCANCEL | MB_ICONWARNING) == IDOK }
}

fn describe(report: &Report, remote: bool) -> (String, String) {
    let chats = |n: usize| {
        if n == 1 {
            "1 chat".to_owned()
        } else {
            format!("{n} chats")
        }
    };
    let light = match report.status.light {
        _ if report.status.finished_at.is_some() => "A chat just finished".to_owned(),
        Light::Off => "No active chats".to_owned(),
        Light::Idle => format!("Idle ({})", chats(report.status.chats)),
        Light::Busy => format!("Working ({} of {})", report.status.working, chats(report.status.chats)),
        Light::Attention => "Needs your input".to_owned(),
    };
    let status = match (report.paused, remote) {
        (false, false) => light,
        (true, false) => "Paused".to_owned(),
        (false, true) => format!("Light: {light}"),
        (true, true) => format!("Light: {light} (this server paused)"),
    };
    let device = match (remote, report.reachable, report.device) {
        (true, true, _) => "PC with the light: reachable".to_owned(),
        (true, false, _) => "PC with the light: NOT reachable".to_owned(),
        (false, _, Some(name)) => format!("Light: {name}"),
        (false, _, None) => "Light: not connected".to_owned(),
    };
    (status, device)
}

impl App {
    fn icon_data(&self) -> NOTIFYICONDATAW {
        let report = self.shared.report();
        let mut data: NOTIFYICONDATAW = unsafe { zeroed() };
        data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = self.hwnd;
        data.uID = TRAY_ID;
        data.uFlags = NIF_ICON | NIF_TIP;
        data.hIcon = self.icons.for_report(&report, self.shared.remote);
        let (status, device) = describe(&report, self.shared.remote);
        let title = if self.shared.remote {
            "Copilot Busylight (server)"
        } else {
            "Copilot Busylight"
        };
        let tip: Vec<u16> = format!("{title}\n{status}\n{device}").encode_utf16().collect();
        let len = tip.len().min(data.szTip.len() - 1);
        data.szTip[..len].copy_from_slice(&tip[..len]);
        data
    }

    fn add_icon(&mut self) {
        let mut data = self.icon_data();
        data.uFlags |= NIF_MESSAGE;
        data.uCallbackMessage = WM_TRAY;
        unsafe { Shell_NotifyIconW(NIM_ADD, &data) };
    }

    fn refresh_icon(&mut self) {
        let data = self.icon_data();
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
    }

    fn run_command(&mut self, command: i32) {
        match command {
            CMD_PAUSE => {
                let paused = !self.shared.paused.load(Relaxed);
                self.shared.paused.store(paused, Relaxed);
                if self.shared.remote {
                    let _ = settings::set_reporting_paused(paused);
                    if paused {
                        self.clear_own_sessions();
                    }
                }
            }
            CMD_FOLDER => self.choose_folder(),
            CMD_AUTOSTART => {
                autostart::set_enabled(!autostart::is_enabled());
            }
            CMD_QUIT => {
                self.shutdown();
                unsafe { DestroyWindow(self.hwnd) };
                return;
            }
            _ => {}
        }
        // Reports that arrived while the menu was open were skipped.
        self.refresh_icon();
    }

    fn choose_folder(&mut self) {
        let remote = self.shared.remote;
        let current = self.shared.sessions_dir();
        let title = if remote {
            "Select a shared folder the PC with the Busylight can also reach (the same folder as the one set on the PC). You can type a path."
        } else {
            "Folder for chat states (the hook writes here, this app reads it):"
        };
        let Some(chosen) = folder_dialog::browse(self.hwnd, title, &current) else {
            return;
        };
        if settings::same_path(&chosen, &current) || !confirm_folder_change(self.hwnd, &chosen) {
            return;
        }
        if settings::set_sessions_dir(&chosen, remote).is_ok() {
            if remote {
                // The old folder would keep this server's chats until they time out.
                self.clear_own_sessions();
            }
            self.shared.set_sessions_dir(chosen);
        }
    }

    /// Removes the state files this server wrote, so the light stops showing them.
    fn clear_own_sessions(&self) {
        let prefix = settings::file_prefix(true);
        let Ok(entries) = std::fs::read_dir(self.shared.sessions_dir()) else {
            return;
        };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with(&prefix) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    /// Stops the worker (which turns the light off) and removes the icon.
    fn shutdown(&mut self) {
        self.shared.stop.store(true, Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let data = self.icon_data();
        unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
    }
}
