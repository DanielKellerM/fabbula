// Copyright 2026 Daniel Keller <daniel.keller.m@gmail.com>
// Licensed under the Apache License, Version 2.0.
// SPDX-License-Identifier: Apache-2.0

use crate::artwork::ArtworkBitmap;

const GLYPH_W: u32 = 8;
const GLYPH_H: u32 = 16;

fn glyph8(ch: char) -> [u8; 8] {
    match ch {
        ' ' => [0, 0, 0, 0, 0, 0, 0, 0],
        '-' => [0, 0, 0, 0b01111100, 0, 0, 0, 0],
        '_' => [0, 0, 0, 0, 0, 0, 0, 0b01111100],
        '.' => [0, 0, 0, 0, 0, 0, 0b00011000, 0],
        ':' => [0, 0b00011000, 0, 0, 0, 0b00011000, 0, 0],
        '/' => [
            0b00000110, 0b00001100, 0b00011000, 0b00110000, 0b01100000, 0b11000000, 0, 0,
        ],
        '?' => [
            0b00111100, 0b01100110, 0b00000110, 0b00001100, 0b00011000, 0, 0b00011000, 0,
        ],
        '0' => [
            0b00111100, 0b01100110, 0b01101110, 0b01110110, 0b01100110, 0b01100110, 0b00111100, 0,
        ],
        '1' => [
            0b00011000, 0b00111000, 0b00011000, 0b00011000, 0b00011000, 0b00011000, 0b00111100, 0,
        ],
        '2' => [
            0b00111100, 0b01100110, 0b00000110, 0b00001100, 0b00110000, 0b01100000, 0b01111110, 0,
        ],
        '3' => [
            0b00111100, 0b01100110, 0b00000110, 0b00011100, 0b00000110, 0b01100110, 0b00111100, 0,
        ],
        '4' => [
            0b00001100, 0b00011100, 0b00101100, 0b01001100, 0b01111110, 0b00001100, 0b00001100, 0,
        ],
        '5' => [
            0b01111110, 0b01100000, 0b01111100, 0b00000110, 0b00000110, 0b01100110, 0b00111100, 0,
        ],
        '6' => [
            0b00011100, 0b00110000, 0b01100000, 0b01111100, 0b01100110, 0b01100110, 0b00111100, 0,
        ],
        '7' => [
            0b01111110, 0b01100110, 0b00000110, 0b00001100, 0b00011000, 0b00011000, 0b00011000, 0,
        ],
        '8' => [
            0b00111100, 0b01100110, 0b01100110, 0b00111100, 0b01100110, 0b01100110, 0b00111100, 0,
        ],
        '9' => [
            0b00111100, 0b01100110, 0b01100110, 0b00111110, 0b00000110, 0b00001100, 0b00111000, 0,
        ],
        'A' => [
            0b00011000, 0b00111100, 0b01100110, 0b01100110, 0b01111110, 0b01100110, 0b01100110, 0,
        ],
        'B' => [
            0b01111100, 0b01100110, 0b01100110, 0b01111100, 0b01100110, 0b01100110, 0b01111100, 0,
        ],
        'C' => [
            0b00111100, 0b01100110, 0b01100000, 0b01100000, 0b01100000, 0b01100110, 0b00111100, 0,
        ],
        'D' => [
            0b01111000, 0b01101100, 0b01100110, 0b01100110, 0b01100110, 0b01101100, 0b01111000, 0,
        ],
        'E' => [
            0b01111110, 0b01100000, 0b01100000, 0b01111100, 0b01100000, 0b01100000, 0b01111110, 0,
        ],
        'F' => [
            0b01111110, 0b01100000, 0b01100000, 0b01111100, 0b01100000, 0b01100000, 0b01100000, 0,
        ],
        'G' => [
            0b00111100, 0b01100110, 0b01100000, 0b01101110, 0b01100110, 0b01100110, 0b00111110, 0,
        ],
        'H' => [
            0b01100110, 0b01100110, 0b01100110, 0b01111110, 0b01100110, 0b01100110, 0b01100110, 0,
        ],
        'I' => [
            0b00111100, 0b00011000, 0b00011000, 0b00011000, 0b00011000, 0b00011000, 0b00111100, 0,
        ],
        'J' => [
            0b00011110, 0b00001100, 0b00001100, 0b00001100, 0b01101100, 0b01101100, 0b00111000, 0,
        ],
        'K' => [
            0b01100110, 0b01101100, 0b01111000, 0b01110000, 0b01111000, 0b01101100, 0b01100110, 0,
        ],
        'L' => [
            0b01100000, 0b01100000, 0b01100000, 0b01100000, 0b01100000, 0b01100000, 0b01111110, 0,
        ],
        'M' => [
            0b01100011, 0b01110111, 0b01111111, 0b01101011, 0b01100011, 0b01100011, 0b01100011, 0,
        ],
        'N' => [
            0b01100110, 0b01110110, 0b01111110, 0b01111110, 0b01101110, 0b01100110, 0b01100110, 0,
        ],
        'O' => [
            0b00111100, 0b01100110, 0b01100110, 0b01100110, 0b01100110, 0b01100110, 0b00111100, 0,
        ],
        'P' => [
            0b01111100, 0b01100110, 0b01100110, 0b01111100, 0b01100000, 0b01100000, 0b01100000, 0,
        ],
        'Q' => [
            0b00111100, 0b01100110, 0b01100110, 0b01100110, 0b01101110, 0b00111100, 0b00000110, 0,
        ],
        'R' => [
            0b01111100, 0b01100110, 0b01100110, 0b01111100, 0b01111000, 0b01101100, 0b01100110, 0,
        ],
        'S' => [
            0b00111110, 0b01100000, 0b01100000, 0b00111100, 0b00000110, 0b00000110, 0b01111100, 0,
        ],
        'T' => [
            0b01111110, 0b01011010, 0b00011000, 0b00011000, 0b00011000, 0b00011000, 0b00111100, 0,
        ],
        'U' => [
            0b01100110, 0b01100110, 0b01100110, 0b01100110, 0b01100110, 0b01100110, 0b00111100, 0,
        ],
        'V' => [
            0b01100110, 0b01100110, 0b01100110, 0b01100110, 0b01100110, 0b00111100, 0b00011000, 0,
        ],
        'W' => [
            0b01100011, 0b01100011, 0b01100011, 0b01101011, 0b01111111, 0b01110111, 0b01100011, 0,
        ],
        'X' => [
            0b01100011, 0b01100011, 0b00110110, 0b00011100, 0b00110110, 0b01100011, 0b01100011, 0,
        ],
        'Y' => [
            0b01100110, 0b01100110, 0b00111100, 0b00011000, 0b00011000, 0b00011000, 0b00111100, 0,
        ],
        'Z' => [
            0b01111110, 0b00000110, 0b00001100, 0b00011000, 0b00110000, 0b01100000, 0b01111110, 0,
        ],
        _ => glyph8('?'),
    }
}

fn glyph16(ch: char) -> [u8; 16] {
    let src = glyph8(ch);
    let mut out = [0u8; 16];
    for (row, byte) in src.into_iter().enumerate() {
        let dst = row * 2;
        out[dst] = byte;
        out[dst + 1] = byte;
    }
    out
}

#[must_use = "rendered text bitmap should be composited or converted to polygons"]
pub fn render_text(text: &str, scale: u32, char_spacing: u32, line_spacing: u32) -> ArtworkBitmap {
    let scale = scale.max(1);
    let lines: Vec<&str> = text.split('\n').collect();
    let line_count = lines.len().max(1) as u32;

    let line_width = |line: &str| -> u32 {
        let chars = line.chars().count() as u32;
        if chars == 0 {
            0
        } else {
            chars * GLYPH_W * scale + (chars - 1) * char_spacing
        }
    };

    let width = lines
        .iter()
        .map(|line| line_width(line))
        .max()
        .unwrap_or(0)
        .max(1);
    let height = line_count * GLYPH_H * scale + (line_count - 1) * line_spacing;

    let mut bitmap = ArtworkBitmap::new_zeroed(width, height.max(1));

    for (li, line) in lines.iter().enumerate() {
        let mut x = 0u32;
        let y0 = (li as u32) * (GLYPH_H * scale + line_spacing);
        for ch in line.chars() {
            let glyph = glyph16(if ch.is_ascii() {
                ch.to_ascii_uppercase()
            } else {
                '?'
            });
            for (gy, row_bits) in glyph.into_iter().enumerate() {
                for gx in 0..GLYPH_W {
                    if (row_bits >> (7 - gx)) & 1 == 1 {
                        let px = x + gx * scale;
                        let py = y0 + gy as u32 * scale;
                        for sy in 0..scale {
                            for sx in 0..scale {
                                bitmap.set(px + sx, py + sy, true);
                            }
                        }
                    }
                }
            }
            x += GLYPH_W * scale + char_spacing;
        }
    }

    bitmap
}

#[cfg(test)]
mod tests {
    use super::render_text;

    fn bitmaps_equal(a: &crate::artwork::ArtworkBitmap, b: &crate::artwork::ArtworkBitmap) -> bool {
        if a.width != b.width || a.height != b.height {
            return false;
        }
        for y in 0..a.height {
            for x in 0..a.width {
                if a.get(x, y) != b.get(x, y) {
                    return false;
                }
            }
        }
        true
    }

    #[test]
    fn test_render_single_char_dimensions() {
        let bmp = render_text("A", 1, 0, 2);
        assert_eq!(bmp.width, 8);
        assert_eq!(bmp.height, 16);
        assert!(bmp.metal_count() > 0);
    }

    #[test]
    fn test_render_string_dimensions() {
        let bmp = render_text("HELLO", 1, 0, 2);
        assert_eq!(bmp.width, 40);
        assert_eq!(bmp.height, 16);
    }

    #[test]
    fn test_render_scale_2() {
        let bmp = render_text("A", 2, 0, 2);
        assert_eq!(bmp.width, 16);
        assert_eq!(bmp.height, 32);
    }

    #[test]
    fn test_render_multiline_dimensions() {
        let bmp = render_text("A\nB", 1, 0, 2);
        assert_eq!(bmp.width, 8);
        assert_eq!(bmp.height, 34);
    }

    #[test]
    fn test_render_char_spacing() {
        let a = render_text("AB", 1, 0, 2);
        let b = render_text("AB", 1, 3, 2);
        assert_eq!(a.width + 3, b.width);
    }

    #[test]
    fn test_render_line_spacing() {
        let a = render_text("A\nB", 1, 0, 2);
        let b = render_text("A\nB", 1, 0, 5);
        assert_eq!(a.height + 3, b.height);
    }

    #[test]
    fn test_render_non_ascii_fallback() {
        let bmp = render_text("ä", 1, 0, 2);
        assert!(bmp.metal_count() > 0);
    }

    #[test]
    fn test_render_empty_string() {
        let bmp = render_text("", 1, 0, 2);
        assert_eq!(bmp.width, 1);
        assert_eq!(bmp.height, 16);
    }

    #[test]
    fn test_render_space_char_is_empty() {
        let bmp = render_text(" ", 1, 0, 2);
        assert_eq!(bmp.width, 8);
        assert_eq!(bmp.height, 16);
        assert_eq!(bmp.metal_count(), 0);
    }

    #[test]
    fn test_lowercase_maps_to_uppercase() {
        let lower = render_text("abcxyz", 1, 1, 2);
        let upper = render_text("ABCXYZ", 1, 1, 2);
        assert!(bitmaps_equal(&lower, &upper));
    }

    #[test]
    fn test_render_all_supported_glyph_branches() {
        let text = " -_.:/?0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let bmp = render_text(text, 1, 0, 2);
        assert!(bmp.width > 0);
        assert_eq!(bmp.height, 16);
        assert!(bmp.metal_count() > 0);
    }
}
