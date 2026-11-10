//! The Windows folder picker, opened at a given folder.

use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree};
use windows_sys::Win32::UI::Shell::{
    BFFM_INITIALIZED, BFFM_SETSELECTIONW, BIF_EDITBOX, BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW,
    SHBrowseForFolderW, SHGetPathFromIDListW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW;

use crate::wide;

/// Returns the chosen folder, or `None` if cancelled. The edit box accepts typed
/// paths, including `\\tsclient\...` ones.
pub fn browse(owner: HWND, title: &str, start: &Path) -> Option<PathBuf> {
    let title = wide(title);
    let start = wide(&start.to_string_lossy());
    let mut display = [0u16; 260];
    let mut path = [0u16; 260];
    unsafe {
        // The new-style dialog needs COM; repeated calls on a thread are harmless.
        CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32);
        let info = BROWSEINFOW {
            hwndOwner: owner,
            pidlRoot: null_mut(),
            pszDisplayName: display.as_mut_ptr(),
            lpszTitle: title.as_ptr(),
            ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE | BIF_EDITBOX,
            lpfn: Some(select_start),
            lParam: start.as_ptr() as LPARAM,
            iImage: 0,
        };
        let chosen = SHBrowseForFolderW(&info);
        if chosen.is_null() {
            return None;
        }
        let ok = SHGetPathFromIDListW(chosen, path.as_mut_ptr()) != 0;
        CoTaskMemFree(chosen.cast());
        if !ok {
            return None;
        }
    }
    let len = path.iter().position(|&c| c == 0).unwrap_or(path.len());
    Some(PathBuf::from(String::from_utf16_lossy(&path[..len])))
}

unsafe extern "system" fn select_start(hwnd: HWND, message: u32, _lparam: LPARAM, start: LPARAM) -> i32 {
    if message == BFFM_INITIALIZED {
        unsafe { SendMessageW(hwnd, BFFM_SETSELECTIONW, 1, start) };
    }
    0
}
