//! Finds a Kuando Busylight among the HID devices and writes commands to it.

use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HIDD_ATTRIBUTES, HIDP_CAPS, HIDP_STATUS_SUCCESS, HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetHidGuid,
    HidD_GetPreparsedData, HidP_GetCaps,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_IO_PENDING, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Threading::{CreateEventW, ResetEvent, WaitForSingleObject};

use crate::kuando;

const WRITE_TIMEOUT_MS: u32 = 1000;

pub struct Device {
    handle: HANDLE,
    event: HANDLE,
    report_len: usize,
    pub name: &'static str,
}

// The handles are only used from the thread that owns the Device.
unsafe impl Send for Device {}

impl Device {
    pub fn open_first() -> Option<Device> {
        hid_paths().iter().find_map(|path| open(path))
    }

    pub fn write(&mut self, packet: &[u8; kuando::PACKET_LEN]) -> bool {
        // Byte 0 is the HID report ID.
        let mut report = vec![0u8; self.report_len];
        report[1..=kuando::PACKET_LEN].copy_from_slice(packet);
        unsafe {
            let mut overlapped: OVERLAPPED = zeroed();
            overlapped.hEvent = self.event;
            ResetEvent(self.event);
            let mut written = 0u32;
            let started = WriteFile(
                self.handle,
                report.as_ptr(),
                report.len() as u32,
                null_mut(),
                &mut overlapped,
            );
            if started == 0 && GetLastError() != ERROR_IO_PENDING {
                return false;
            }
            if WaitForSingleObject(self.event, WRITE_TIMEOUT_MS) != WAIT_OBJECT_0 {
                CancelIoEx(self.handle, &overlapped);
                GetOverlappedResult(self.handle, &overlapped, &mut written, 1);
                return false;
            }
            GetOverlappedResult(self.handle, &overlapped, &mut written, 0) != 0
        }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
            CloseHandle(self.event);
        }
    }
}

/// NUL-terminated device paths of all present HID interfaces.
fn hid_paths() -> Vec<Vec<u16>> {
    let mut paths = Vec::new();
    unsafe {
        let mut guid = zeroed();
        HidD_GetHidGuid(&mut guid);
        let set = SetupDiGetClassDevsW(&guid, null(), null_mut(), DIGCF_PRESENT | DIGCF_DEVICEINTERFACE);
        if set == INVALID_HANDLE_VALUE as isize {
            return paths;
        }
        for index in 0.. {
            let mut interface: SP_DEVICE_INTERFACE_DATA = zeroed();
            interface.cbSize = size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
            if SetupDiEnumDeviceInterfaces(set, null(), &guid, index, &mut interface) == 0 {
                break;
            }
            let mut needed = 0u32;
            SetupDiGetDeviceInterfaceDetailW(set, &interface, null_mut(), 0, &mut needed, null_mut());
            if needed == 0 {
                continue;
            }
            // u32 elements keep the buffer aligned for the detail struct.
            let mut buffer = vec![0u32; (needed as usize).div_ceil(4)];
            let detail = buffer.as_mut_ptr().cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
            (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(set, &interface, detail, needed, null_mut(), null_mut()) == 0 {
                continue;
            }
            let start = (&raw const (*detail).DevicePath).cast::<u16>();
            let mut len = 0;
            while *start.add(len) != 0 {
                len += 1;
            }
            let mut path = std::slice::from_raw_parts(start, len).to_vec();
            path.push(0);
            paths.push(path);
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    paths
}

fn open(path: &[u16]) -> Option<Device> {
    unsafe {
        // Zero access is enough to read the IDs, even for devices others hold open.
        let probe = CreateFileW(
            path.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            0,
            null_mut(),
        );
        if probe == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut attributes: HIDD_ATTRIBUTES = zeroed();
        attributes.Size = size_of::<HIDD_ATTRIBUTES>() as u32;
        let known = HidD_GetAttributes(probe, &mut attributes) != 0
            && kuando::DEVICE_IDS.contains(&(attributes.VendorID, attributes.ProductID));
        CloseHandle(probe);
        if !known {
            return None;
        }

        let handle = CreateFileW(
            path.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return None;
        }
        let report_len = output_report_len(handle);
        let event = CreateEventW(null(), 1, 0, null());
        if report_len <= kuando::PACKET_LEN || event.is_null() {
            CloseHandle(handle);
            if !event.is_null() {
                CloseHandle(event);
            }
            return None;
        }
        Some(Device {
            handle,
            event,
            report_len,
            name: kuando::model_name(attributes.ProductID),
        })
    }
}

fn output_report_len(handle: HANDLE) -> usize {
    unsafe {
        let mut data = zeroed();
        if HidD_GetPreparsedData(handle, &mut data) == 0 {
            return 0;
        }
        let mut caps: HIDP_CAPS = zeroed();
        let status = HidP_GetCaps(data, &mut caps);
        HidD_FreePreparsedData(data);
        if status == HIDP_STATUS_SUCCESS {
            usize::from(caps.OutputReportByteLength)
        } else {
            0
        }
    }
}
