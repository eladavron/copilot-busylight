//! Tray icons drawn at runtime: a coloured dot per state.

use std::mem::{size_of, zeroed};
use std::ptr::null_mut;

use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS, DeleteObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, DestroyIcon, GetSystemMetrics, HICON, ICONINFO, SM_CXSMICON,
};

use crate::light::{ATTENTION_COLOR, BUSY_COLOR, IDLE_COLOR, Report};
use crate::sessions::Light;

const GREY: (u8, u8, u8) = (150, 150, 150);

pub struct Icons {
    off: HICON,
    idle: HICON,
    busy: HICON,
    attention: HICON,
    paused: HICON,
}

impl Icons {
    pub fn new() -> Icons {
        let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16) as usize;
        Icons {
            off: make(size, GREY, true),
            idle: make(size, IDLE_COLOR, false),
            busy: make(size, BUSY_COLOR, false),
            attention: make(size, ATTENTION_COLOR, false),
            paused: make(size, GREY, false),
        }
    }

    /// A paused server still mirrors the light, which other machines keep driving.
    pub fn for_report(&self, report: &Report, remote: bool) -> HICON {
        if report.paused && !remote {
            return self.paused;
        }
        if report.status.finished_at.is_some() {
            return self.idle;
        }
        match report.status.light {
            Light::Off => self.off,
            Light::Idle => self.idle,
            Light::Busy => self.busy,
            Light::Attention => self.attention,
        }
    }
}

impl Drop for Icons {
    fn drop(&mut self) {
        for icon in [self.off, self.idle, self.busy, self.attention, self.paused] {
            unsafe { DestroyIcon(icon) };
        }
    }
}

/// Anti-aliased disc, or a ring when `ring` is set, as 32-bit BGRA pixels.
fn pixels(size: usize, (r, g, b): (u8, u8, u8), ring: bool) -> Vec<u32> {
    let center = size as f32 / 2.0;
    let outer = center - 1.0;
    let inner = if ring {
        outer - (size as f32 / 6.0).max(1.5)
    } else {
        -1.0
    };
    let rgb = u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b);
    (0..size * size)
        .map(|i| {
            let (x, y) = ((i % size) as f32 + 0.5 - center, (i / size) as f32 + 0.5 - center);
            let distance = (x * x + y * y).sqrt();
            let cover = (outer - distance + 0.5).clamp(0.0, 1.0) * (distance - inner + 0.5).clamp(0.0, 1.0);
            ((cover * 255.0).round() as u32) << 24 | rgb
        })
        .collect()
}

fn make(size: usize, rgb: (u8, u8, u8), ring: bool) -> HICON {
    let pixels = pixels(size, rgb, ring);
    unsafe {
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = size as i32;
        // Negative height: top-down rows.
        info.bmiHeader.biHeight = -(size as i32);
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut bits = null_mut();
        let color = CreateDIBSection(null_mut(), &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if color.is_null() || bits.is_null() {
            return null_mut();
        }
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u32>(), pixels.len());

        // All-zero AND mask: transparency comes from the alpha channel.
        let mask_bits = vec![0u8; size.div_ceil(16) * 2 * size];
        let mask = CreateBitmap(size as i32, size as i32, 1, 1, mask_bits.as_ptr().cast());
        let icon_info = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&icon_info);
        DeleteObject(color);
        DeleteObject(mask);
        icon
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(pixel: u32) -> u32 {
        pixel >> 24
    }

    #[test]
    fn disc_and_ring_shapes() {
        let size = 16;
        let disc = pixels(size, (255, 0, 0), false);
        let ring = pixels(size, (255, 0, 0), true);
        let center = size / 2 * size + size / 2;
        assert_eq!(alpha(disc[center]), 255);
        assert_eq!(alpha(ring[center]), 0);
        assert_eq!(alpha(disc[0]), 0);
        assert_eq!(disc[center] & 0x00FF_FFFF, 0x00FF_0000);
    }
}
