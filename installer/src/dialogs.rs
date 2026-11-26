//! Message boxes, and the dialog that picks which agents to install hooks for.

use std::cell::RefCell;
use std::path::Path;
use std::ptr::{null, null_mut};

use agent_busylight::harness::Harness;
use agent_busylight::{APP_NAME, wide};
use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::System::ApplicationInstallationAndServicing::{ACTCTXW, CreateActCtxW};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{BST_CHECKED, BST_UNCHECKED, CheckDlgButton, IsDlgButtonChecked};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, BS_PUSHBUTTON, DLGTEMPLATE, DS_CENTER, DS_MODALFRAME, DS_SETFONT,
    DS_SETFOREGROUND, DialogBoxIndirectParamW, EndDialog, GetDlgItem, IDCANCEL, IDNO, IDOK, IDYES, MB_ICONERROR,
    MB_ICONINFORMATION, MB_ICONQUESTION, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_YESNO, MB_YESNOCANCEL,
    MESSAGEBOX_STYLE, MessageBoxW, WM_CLOSE, WM_COMMAND, WM_INITDIALOG, WS_CAPTION, WS_CHILD, WS_POPUP, WS_SYSMENU,
    WS_TABSTOP, WS_VISIBLE,
};

const ACTCTX_FLAG_ASSEMBLY_DIRECTORY_VALID: u32 = 0x4;
const ACTCTX_FLAG_RESOURCE_NAME_VALID: u32 = 0x8;
const ACTCTX_FLAG_SET_PROCESS_DEFAULT: u32 = 0x10;
/// shell32.dll's own manifest, which asks for Common Controls 6 (visual styles).
const SHELL32_MANIFEST_ID: usize = 124;
const BUTTON_CLASS: u16 = 0x0080;
const STATIC_CLASS: u16 = 0x0082;
const FIRST_CHECKBOX: i32 = 100;

pub enum Answer {
    Local,
    Remote,
    Cancel,
}

fn title() -> String {
    format!("{APP_NAME} Setup")
}

fn show(text: &str, style: MESSAGEBOX_STYLE) -> i32 {
    let (text, title) = (wide(text), wide(&title()));
    unsafe { MessageBoxW(null_mut(), text.as_ptr(), title.as_ptr(), style | MB_SETFOREGROUND) }
}

/// Gives the dialogs today's look instead of Windows 95's: the exe has no
/// manifest of its own, so this borrows shell32's.
pub fn use_visual_styles() {
    let mut system_dir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
    system_dir.push_str(r"\System32");
    let (source, dir) = (wide("shell32.dll"), wide(&system_dir));
    let context = ACTCTXW {
        cbSize: size_of::<ACTCTXW>() as u32,
        dwFlags: ACTCTX_FLAG_RESOURCE_NAME_VALID
            | ACTCTX_FLAG_SET_PROCESS_DEFAULT
            | ACTCTX_FLAG_ASSEMBLY_DIRECTORY_VALID,
        lpSource: source.as_ptr(),
        wProcessorArchitecture: 0,
        wLangId: 0,
        lpAssemblyDirectory: dir.as_ptr(),
        lpResourceName: SHELL32_MANIFEST_ID as *const u16,
        lpApplicationName: null(),
        hModule: null_mut(),
    };
    // A failure only means the old look.
    unsafe { CreateActCtxW(&context) };
}

pub fn ask_mode() -> Answer {
    let answer = show(
        "Is the Busylight plugged into THIS computer?\n\n\
         Yes - install the tray app that drives the light, and the agent hooks.\n\n\
         No - this is a remote server you reach over Remote Desktop. The hooks \
         drive the light on your PC through RDP drive sharing, and the tray icon \
         here shows what the light shows.",
        MB_YESNOCANCEL | MB_ICONQUESTION,
    );
    match answer {
        IDYES => Answer::Local,
        IDNO => Answer::Remote,
        IDCANCEL => Answer::Cancel,
        _ => Answer::Cancel,
    }
}

/// Server only. Yes keeps the folder, No opens the folder picker, Cancel aborts setup.
pub fn ask_keep_folder(dir: &Path) -> Option<bool> {
    let answer = show(
        &format!(
            "The PC with the Busylight must also be able to read this folder: select a shared \
             folder both can reach (it must be the same folder as the one set on the PC).\n\n\
             {}\n\nUse this folder?\n\nYes - use it.\nNo - choose a different folder.",
            dir.display()
        ),
        MB_YESNOCANCEL | MB_ICONQUESTION,
    );
    match answer {
        IDYES => Some(true),
        IDNO => Some(false),
        _ => None,
    }
}

pub fn confirm(text: &str) -> bool {
    show(text, MB_YESNO | MB_ICONQUESTION) == IDYES
}

pub fn info(text: &str) {
    show(text, MB_OK | MB_ICONINFORMATION);
}

pub fn warning(text: &str) {
    show(text, MB_OK | MB_ICONWARNING);
}

pub fn error(text: &str) {
    show(text, MB_OK | MB_ICONERROR);
}

thread_local! {
    /// The checkboxes' state, shared with the dialog procedure.
    static CHECKED: RefCell<Vec<bool>> = const { RefCell::new(Vec::new()) };
}

/// Checkboxes for the agents, ticked as in `checked`; `None` if cancelled.
pub fn choose_harnesses(checked: &[Harness]) -> Option<Vec<Harness>> {
    let labels: Vec<String> = Harness::ALL
        .iter()
        .map(|harness| {
            let state = if harness.is_detected() { "found" } else { "not found" };
            format!("{} ({state})", harness.name())
        })
        .collect();
    CHECKED.with(|state| *state.borrow_mut() = Harness::ALL.iter().map(|h| checked.contains(h)).collect());
    let template = harness_template(&labels);
    let result = unsafe {
        DialogBoxIndirectParamW(
            GetModuleHandleW(null()),
            template.as_ptr().cast::<DLGTEMPLATE>(),
            null_mut(),
            Some(harness_dialog_proc),
            0,
        )
    };
    if result != IDOK as isize {
        return None;
    }
    let state = CHECKED.with(|state| state.borrow().clone());
    Some(
        Harness::ALL
            .into_iter()
            .zip(state)
            .filter_map(|(harness, on)| on.then_some(harness))
            .collect(),
    )
}

unsafe extern "system" fn harness_dialog_proc(dialog: HWND, message: u32, wparam: WPARAM, _: LPARAM) -> isize {
    let count = Harness::ALL.len() as i32;
    let read = || (0..count).map(|i| unsafe { IsDlgButtonChecked(dialog, FIRST_CHECKBOX + i) } == BST_CHECKED);
    let update_ok = |any: bool| unsafe { EnableWindow(GetDlgItem(dialog, IDOK), i32::from(any)) };
    match message {
        WM_INITDIALOG => {
            let state = CHECKED.with(|state| state.borrow().clone());
            for (i, &on) in state.iter().enumerate() {
                let check = if on { BST_CHECKED } else { BST_UNCHECKED };
                unsafe { CheckDlgButton(dialog, FIRST_CHECKBOX + i as i32, check) };
            }
            update_ok(state.contains(&true));
            1
        }
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as i32;
            match id {
                IDOK => {
                    let state: Vec<bool> = read().collect();
                    CHECKED.with(|slot| *slot.borrow_mut() = state);
                    unsafe { EndDialog(dialog, IDOK as isize) };
                }
                IDCANCEL => {
                    unsafe { EndDialog(dialog, IDCANCEL as isize) };
                }
                _ if (FIRST_CHECKBOX..FIRST_CHECKBOX + count).contains(&id) => {
                    update_ok(read().any(|on| on));
                }
                _ => return 0,
            }
            1
        }
        WM_CLOSE => {
            unsafe { EndDialog(dialog, IDCANCEL as isize) };
            1
        }
        _ => 0,
    }
}

/// An in-memory dialog template (the exe has no resources). Sizes are in dialog units.
fn harness_template(labels: &[String]) -> Vec<u32> {
    const WIDTH: i16 = 240;
    const MARGIN: i16 = 7;
    const ROW: i16 = 14;
    let intro = "Which coding agents should drive the light? Setup adds its hooks to each one ticked. \
                 You can change this later from the tray menu (Hooks).";
    let checks_top = MARGIN + 36;
    let buttons_top = checks_top + ROW * labels.len() as i16 + 8;
    let height = buttons_top + 14 + MARGIN;

    let mut t = Template::default();
    let style = WS_POPUP | WS_CAPTION | WS_SYSMENU | (DS_MODALFRAME | DS_SETFONT | DS_CENTER | DS_SETFOREGROUND) as u32;
    t.dword(style);
    t.dword(0);
    t.word(labels.len() as u16 + 3);
    for value in [0, 0, WIDTH, height] {
        t.word(value as u16);
    }
    t.word(0); // no menu
    t.word(0); // standard dialog class
    t.text(&title());
    t.word(9);
    t.text("Segoe UI");

    let child = WS_CHILD | WS_VISIBLE;
    t.item(
        child,
        [MARGIN, MARGIN, WIDTH - 2 * MARGIN, 30],
        0xFFFF,
        STATIC_CLASS,
        intro,
    );
    for (i, label) in labels.iter().enumerate() {
        let top = checks_top + ROW * i as i16;
        let id = (FIRST_CHECKBOX + i as i32) as u16;
        let style = child | WS_TABSTOP | BS_AUTOCHECKBOX as u32;
        t.item(
            style,
            [MARGIN * 2, top, WIDTH - 4 * MARGIN, 10],
            id,
            BUTTON_CLASS,
            label,
        );
    }
    let ok = child | WS_TABSTOP | BS_DEFPUSHBUTTON as u32;
    let cancel = child | WS_TABSTOP | BS_PUSHBUTTON as u32;
    t.item(
        ok,
        [WIDTH - MARGIN - 50 - 4 - 50, buttons_top, 50, 14],
        IDOK as u16,
        BUTTON_CLASS,
        "OK",
    );
    t.item(
        cancel,
        [WIDTH - MARGIN - 50, buttons_top, 50, 14],
        IDCANCEL as u16,
        BUTTON_CLASS,
        "Cancel",
    );
    t.finish()
}

#[derive(Default)]
struct Template(Vec<u16>);

impl Template {
    fn word(&mut self, value: u16) {
        self.0.push(value);
    }

    fn dword(&mut self, value: u32) {
        self.0.extend([value as u16, (value >> 16) as u16]);
    }

    fn text(&mut self, text: &str) {
        self.0.extend(wide(text));
    }

    /// One control: DLGITEMTEMPLATE, a predefined class atom, its text, no creation data.
    fn item(&mut self, style: u32, [x, y, cx, cy]: [i16; 4], id: u16, class: u16, text: &str) {
        if self.0.len() % 2 == 1 {
            self.word(0); // items start on a DWORD boundary
        }
        self.dword(style);
        self.dword(0);
        for value in [x, y, cx, cy] {
            self.word(value as u16);
        }
        self.word(id);
        self.word(0xFFFF);
        self.word(class);
        self.text(text);
        self.word(0);
    }

    /// DWORD-aligned, as DialogBoxIndirectParamW requires.
    fn finish(self) -> Vec<u32> {
        self.0
            .chunks(2)
            .map(|pair| u32::from(pair[0]) | pair.get(1).map_or(0, |&high| u32::from(high) << 16))
            .collect()
    }
}
