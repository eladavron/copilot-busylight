//! Small helpers for values under HKEY_CURRENT_USER.

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{ERROR_SUCCESS, WIN32_ERROR};
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_DWORD, REG_SZ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegDeleteTreeW,
    RegGetValueW, RegSetKeyValueW,
};

use crate::wide;

fn check(status: WIN32_ERROR) -> Result<(), WIN32_ERROR> {
    if status == ERROR_SUCCESS { Ok(()) } else { Err(status) }
}

pub fn get_string(key: &str, name: &str) -> Option<String> {
    let (key, name) = (wide(key), wide(name));
    unsafe {
        let mut size = 0u32;
        let query = |data: *mut core::ffi::c_void, size: &mut u32| {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                null_mut(),
                data,
                size,
            )
        };
        check(query(null_mut(), &mut size)).ok()?;
        let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
        check(query(buffer.as_mut_ptr().cast(), &mut size)).ok()?;
        let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..len]))
    }
}

pub fn get_dword(key: &str, name: &str) -> Option<u32> {
    let (key, name) = (wide(key), wide(name));
    let mut value = 0u32;
    let mut size = 4u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            (&raw mut value).cast(),
            &mut size,
        )
    };
    check(status).ok().map(|()| value)
}

pub fn set_string(key: &str, name: &str, value: &str) -> Result<(), WIN32_ERROR> {
    let (key, name, value) = (wide(key), wide(name), wide(value));
    check(unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            REG_SZ,
            value.as_ptr().cast(),
            (value.len() * 2) as u32,
        )
    })
}

pub fn set_dword(key: &str, name: &str, value: u32) -> Result<(), WIN32_ERROR> {
    let (key, name) = (wide(key), wide(name));
    check(unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            REG_DWORD,
            (&raw const value).cast(),
            4,
        )
    })
}

pub fn delete_value(key: &str, name: &str) -> bool {
    let (key, name) = (wide(key), wide(name));
    unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) == ERROR_SUCCESS }
}

pub fn delete_key(key: &str) {
    let key = wide(key);
    unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, key.as_ptr()) };
}
