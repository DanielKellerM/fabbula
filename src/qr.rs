// Copyright 2026 Daniel Keller <daniel.keller.m@gmail.com>
// Licensed under the Apache License, Version 2.0.
// SPDX-License-Identifier: Apache-2.0

use crate::artwork::ArtworkBitmap;
use anyhow::Result;
use clap::ValueEnum;
use qrcode::types::Color;
use qrcode::{EcLevel as QrEcLevel, QrCode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum EcLevel {
    L,
    M,
    Q,
    H,
}

impl From<EcLevel> for QrEcLevel {
    fn from(v: EcLevel) -> Self {
        match v {
            EcLevel::L => QrEcLevel::L,
            EcLevel::M => QrEcLevel::M,
            EcLevel::Q => QrEcLevel::Q,
            EcLevel::H => QrEcLevel::H,
        }
    }
}

pub fn render_qr(
    data: &str,
    module_size: u32,
    ec_level: EcLevel,
    quiet_zone: u32,
) -> Result<ArtworkBitmap> {
    anyhow::ensure!(!data.is_empty(), "QR data must not be empty");
    anyhow::ensure!(module_size > 0, "module_size must be >= 1");
    let code = QrCode::with_error_correction_level(data.as_bytes(), ec_level.into())?;
    let width = code.width() as u32;
    let out_side = (width + 2 * quiet_zone) * module_size;
    let mut out = ArtworkBitmap::new_zeroed(out_side, out_side);

    let colors = code.to_colors();
    for y in 0..width {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            if colors[idx] == Color::Dark {
                let ox = (x + quiet_zone) * module_size;
                let oy = (y + quiet_zone) * module_size;
                for dy in 0..module_size {
                    for dx in 0..module_size {
                        out.set(ox + dx, oy + dy, true);
                    }
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{EcLevel, render_qr};

    #[test]
    fn test_render_qr_basic() {
        let bmp = render_qr("hello", 2, EcLevel::M, 4).unwrap();
        assert_eq!(bmp.width, bmp.height);
        assert!(bmp.metal_count() > 0);
    }

    #[test]
    fn test_render_qr_module_size_scales() {
        let a = render_qr("hello", 1, EcLevel::M, 4).unwrap();
        let b = render_qr("hello", 3, EcLevel::M, 4).unwrap();
        assert_eq!(b.width, a.width * 3);
    }

    #[test]
    fn test_render_qr_quiet_zone_changes_size() {
        let a = render_qr("hello", 2, EcLevel::M, 0).unwrap();
        let b = render_qr("hello", 2, EcLevel::M, 4).unwrap();
        assert!(b.width > a.width);
    }
}
