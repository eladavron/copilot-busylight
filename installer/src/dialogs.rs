//! Message boxes. The installer has no other user interface.

use std::path::Path;
use std::ptr::null_mut;

use copilot_busylight::wide;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IDCANCEL, IDNO, IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_SETFOREGROUND, MB_YESNO,
    MB_YESNOCANCEL, MESSAGEBOX_STYLE, MessageBoxW,
};

const TITLE: &str = "Copilot Busylight Setup";

pub enum Answer {
    Local,
    Remote,
    Cancel,
}

fn show(text: &str, style: MESSAGEBOX_STYLE) -> i32 {
    let (text, title) = (wide(text), wide(TITLE));
    unsafe { MessageBoxW(null_mut(), text.as_ptr(), title.as_ptr(), style | MB_SETFOREGROUND) }
}

pub fn ask_mode() -> Answer {
    let answer = show(
        "Is the Busylight plugged into THIS computer?\n\n\
         Yes - install the tray app that drives the light, and the Copilot hooks.\n\n\
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

pub fn error(text: &str) {
    show(text, MB_OK | MB_ICONERROR);
}
