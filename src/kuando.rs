// Copyright 2025 Erik O'Shaughnessy (busylight-core)
// Copyright 2026 Elad Avron
// SPDX-License-Identifier: Apache-2.0
//
// Modified by Elad Avron in 2026: ported from Python to Rust from
// busylight-core's src/busylight_core/vendors/kuando/
// (https://github.com/JnyJny/busylight-core), and reduced to steady colours.

//! Kuando Busylight Alpha/Omega USB protocol, ported from busylight-core.
//!
//! A command is eight big-endian 64-bit words: seven steps and a footer that
//! ends in a 16-bit checksum of all preceding bytes.

/// USB (vendor, product) IDs of supported Busylights.
pub const DEVICE_IDS: &[(u16, u16)] = &[
    (0x04D8, 0xF848),
    (0x27BB, 0x3BCA),
    (0x27BB, 0x3BCB),
    (0x27BB, 0x3BCD),
    (0x27BB, 0x3BCE),
    (0x27BB, 0x3BCF),
];

pub const PACKET_LEN: usize = 64;

const OPCODE_JUMP: u64 = 0x1;
const FOOTER_PAD: u64 = 0xFFF;

pub fn model_name(product_id: u16) -> &'static str {
    if matches!(product_id, 0x3BCD | 0x3BCF) {
        "Busylight Omega"
    } else {
        "Busylight Alpha"
    }
}

/// The device uses a 0-100 scale per channel.
fn scale(channel: u8) -> u64 {
    u64::from(channel) * 100 / 255
}

/// Command that shows a steady colour (0-255 per channel).
pub fn color_packet(rgb: (u8, u8, u8)) -> [u8; PACKET_LEN] {
    let step = OPCODE_JUMP << 60 | scale(rgb.0) << 40 | scale(rgb.1) << 32 | scale(rgb.2) << 24;
    let footer = FOOTER_PAD << 16;

    let mut packet = [0u8; PACKET_LEN];
    packet[..8].copy_from_slice(&step.to_be_bytes());
    packet[56..].copy_from_slice(&footer.to_be_bytes());
    let checksum: u32 = packet[..62].iter().map(|&b| u32::from(b)).sum();
    packet[62..].copy_from_slice(&(checksum as u16).to_be_bytes());
    packet
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(packet: &[u8]) -> String {
        packet.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn expected(step: &str, checksum: &str) -> String {
        format!("{step}{}0fff{checksum}", "0".repeat(104))
    }

    // Reference packets generated with busylight-core 2.5.0.
    #[test]
    fn matches_busylight_core() {
        let cases = [
            ((255, 0, 0), "1000640000000000", "0182"),
            ((0, 255, 0), "1000006400000000", "0182"),
            ((255, 200, 0), "1000644e00000000", "01d0"),
            ((128, 64, 32), "100032190c000000", "0175"),
            ((0, 0, 0), "1000000000000000", "011e"),
        ];
        for (rgb, step, checksum) in cases {
            assert_eq!(hex(&color_packet(rgb)), expected(step, checksum), "{rgb:?}");
        }
    }
}
