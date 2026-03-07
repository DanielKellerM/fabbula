// Copyright 2026 Daniel Keller <daniel.keller.m@gmail.com>
// Licensed under the Apache License, Version 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Image loading, thresholding, and density enforcement.
//!
//! Provides [`load_artwork`] for converting raster images into binary bitmaps,
//! [`apply_exclusion_mask`] for masking out reserved regions, and
//! [`enforce_density`] for satisfying maximum metal density DRC rules.

use crate::pdk::PdkConfig;
use anyhow::{Context, Result};
use image::{DynamicImage, GenericImageView, Luma};
use std::path::Path;

/// A binary bitmap where true = metal, false = gap.
/// Stored as a packed bitset (`Vec<u64>`) for cache efficiency.
#[derive(Debug, Clone)]
pub struct ArtworkBitmap {
    pub width: u32,
    pub height: u32,
    pixels: Vec<u64>,
}

fn num_words(bits: usize) -> usize {
    bits.div_ceil(64)
}

impl ArtworkBitmap {
    /// Create a bitmap from a bool slice, packing into u64 words.
    pub fn from_bools(width: u32, height: u32, bools: &[bool]) -> Self {
        let total = (width as usize) * (height as usize);
        assert_eq!(
            bools.len(),
            total,
            "bools length {} != width*height {}",
            bools.len(),
            total
        );
        let mut pixels = vec![0u64; num_words(total)];
        for (i, &b) in bools.iter().enumerate() {
            if b {
                pixels[i / 64] |= 1u64 << (i % 64);
            }
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    /// Create an all-false bitmap.
    pub fn new_zeroed(width: u32, height: u32) -> Self {
        let total = (width as usize) * (height as usize);
        Self {
            width,
            height,
            pixels: vec![0u64; num_words(total)],
        }
    }

    /// Access the raw packed words (read-only)
    #[inline]
    pub fn words(&self) -> &[u64] {
        &self.pixels
    }

    /// Access the raw packed words (mutable)
    #[inline]
    pub fn words_mut(&mut self) -> &mut [u64] {
        &mut self.pixels
    }

    /// Return the packed word slice and the bit offset of the first pixel for row `y`.
    #[inline]
    pub fn row_words(&self, y: u32) -> (&[u64], usize) {
        let bit_start = (y as usize) * (self.width as usize);
        let bit_end = bit_start + self.width as usize;
        let word_start = bit_start / 64;
        let word_end = bit_end.div_ceil(64);
        (&self.pixels[word_start..word_end], bit_start % 64)
    }

    /// Returns the value of the pixel at `(x, y)`. Out-of-bounds coordinates return `false`.
    #[inline]
    pub fn get(&self, x: u32, y: u32) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let i = (y * self.width + x) as usize;
        self.pixels[i / 64] & (1u64 << (i % 64)) != 0
    }

    /// Sets the pixel at `(x, y)` to `val`. Out-of-bounds coordinates are ignored.
    pub fn set(&mut self, x: u32, y: u32, val: bool) {
        if x < self.width && y < self.height {
            let i = (y * self.width + x) as usize;
            if val {
                self.pixels[i / 64] |= 1u64 << (i % 64);
            } else {
                self.pixels[i / 64] &= !(1u64 << (i % 64));
            }
        }
    }

    /// Count of "on" pixels
    pub fn metal_count(&self) -> usize {
        let total = (self.width as usize) * (self.height as usize);
        let full_words = total / 64;
        let remainder = total % 64;
        let mut count: usize = self.pixels[..full_words]
            .iter()
            .map(|w| w.count_ones() as usize)
            .sum();
        if remainder > 0 {
            // Mask off padding bits in the last word
            let mask = (1u64 << remainder) - 1;
            count += (self.pixels[full_words] & mask).count_ones() as usize;
        }
        count
    }

    /// Metal density as a fraction
    pub fn density(&self) -> f64 {
        self.metal_count() as f64 / (self.width * self.height) as f64
    }

    /// Invert the bitmap (swap metal/gap)
    pub fn invert(&mut self) {
        for w in &mut self.pixels {
            *w = !*w;
        }
        // Clear padding bits in the last word
        let total = (self.width as usize) * (self.height as usize);
        let remainder = total % 64;
        if remainder > 0 {
            let mask = (1u64 << remainder) - 1;
            if let Some(last) = self.pixels.last_mut() {
                *last &= mask;
            }
        }
    }
}

/// How to interpret the image for thresholding
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThresholdMode {
    /// Simple luminance threshold: below = metal, above = gap
    Luminance(u8),
    /// Otsu's automatic thresholding
    Otsu,
    /// Alpha channel: transparent = gap, opaque = metal
    Alpha(u8),
}

/// Check if a file path has an SVG extension.
pub fn is_svg(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("svg") || e.eq_ignore_ascii_case("svgz"))
        .unwrap_or(false)
}

/// Rasterize an SVG file to a `DynamicImage`, fitting within `target_size` if provided.
pub fn rasterize_svg(path: &Path, target_size: Option<(u32, u32)>) -> Result<DynamicImage> {
    let svg_data =
        std::fs::read(path).with_context(|| format!("Failed to read SVG: {}", path.display()))?;

    let tree = resvg::usvg::Tree::from_data(&svg_data, &resvg::usvg::Options::default())
        .with_context(|| format!("Failed to parse SVG: {}", path.display()))?;

    let svg_size = tree.size();
    let svg_w = svg_size.width();
    let svg_h = svg_size.height();

    let (px_w, px_h) = if let Some((tw, th)) = target_size {
        let sx = tw as f32 / svg_w;
        let sy = th as f32 / svg_h;
        let scale = sx.min(sy);
        ((svg_w * scale).ceil() as u32, (svg_h * scale).ceil() as u32)
    } else {
        (svg_w.ceil() as u32, svg_h.ceil() as u32)
    };

    anyhow::ensure!(px_w > 0 && px_h > 0, "SVG rasterizes to zero dimensions");

    let mut pixmap = resvg::tiny_skia::Pixmap::new(px_w, px_h)
        .ok_or_else(|| anyhow::anyhow!("Failed to create pixmap {}x{}", px_w, px_h))?;

    let scale_x = px_w as f32 / svg_w;
    let scale_y = px_h as f32 / svg_h;
    let transform = resvg::tiny_skia::Transform::from_scale(scale_x, scale_y);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // Convert from premultiplied RGBA to straight RGBA
    let pixels = pixmap.data_mut();
    for chunk in pixels.chunks_exact_mut(4) {
        let a = chunk[3] as f32 / 255.0;
        if a > 0.0 && a < 1.0 {
            chunk[0] = (chunk[0] as f32 / a).min(255.0) as u8;
            chunk[1] = (chunk[1] as f32 / a).min(255.0) as u8;
            chunk[2] = (chunk[2] as f32 / a).min(255.0) as u8;
        }
    }

    let rgba_img = image::RgbaImage::from_raw(px_w, px_h, pixels.to_vec())
        .ok_or_else(|| anyhow::anyhow!("Failed to create RGBA image from SVG pixmap"))?;

    tracing::info!(
        "Rasterized SVG {:.0}x{:.0} -> {}x{} px",
        svg_w,
        svg_h,
        px_w,
        px_h
    );
    Ok(DynamicImage::ImageRgba8(rgba_img))
}

/// Apply Floyd-Steinberg dithering to a grayscale image buffer.
///
/// Uses an f32 error buffer to avoid u8 clamping artifacts. The standard FS kernel
/// distributes quantization error: 7/16 right, 3/16 bottom-left, 5/16 bottom, 1/16 bottom-right.
pub fn floyd_steinberg_dither(gray: &mut image::GrayImage, threshold: u8) {
    let w = gray.width() as usize;
    let h = gray.height() as usize;

    let mut buf: Vec<f32> = gray.as_raw().iter().map(|&v| v as f32).collect();

    for y in 0..h {
        for x in 0..w {
            let idx = y * w + x;
            let old = buf[idx];
            let new = if old < threshold as f32 { 0.0 } else { 255.0 };
            buf[idx] = new;
            let err = old - new;

            if x + 1 < w {
                buf[idx + 1] += err * 7.0 / 16.0;
            }
            if y + 1 < h {
                if x > 0 {
                    buf[(y + 1) * w + x - 1] += err * 3.0 / 16.0;
                }
                buf[(y + 1) * w + x] += err * 5.0 / 16.0;
                if x + 1 < w {
                    buf[(y + 1) * w + x + 1] += err * 1.0 / 16.0;
                }
            }
        }
    }

    let raw = gray.as_mut();
    for (px, &val) in raw.iter_mut().zip(buf.iter()) {
        *px = val.clamp(0.0, 255.0) as u8;
    }
}

/// Load an image file (raster or SVG) and open it, returning a DynamicImage.
pub fn load_image_file(path: &Path, max_pixels: Option<(u32, u32)>) -> Result<DynamicImage> {
    let img = if is_svg(path) {
        rasterize_svg(path, max_pixels)?
    } else {
        image::open(path).with_context(|| format!("Failed to open image: {}", path.display()))?
    };
    Ok(img)
}

/// Load an image file and convert to a binary artwork bitmap.
///
/// If `max_pixels` is set, the image is downscaled so that the
/// total pixel count for the artwork doesn't exceed it (useful for
/// keeping GDSII file sizes reasonable).
///
/// If `dither` is true, Floyd-Steinberg dithering is applied before
/// thresholding, producing dot patterns for gradients instead of hard edges.
pub fn load_artwork(
    path: &Path,
    threshold: ThresholdMode,
    max_pixels: Option<(u32, u32)>,
    dither: bool,
) -> Result<ArtworkBitmap> {
    let img = load_image_file(path, if is_svg(path) { max_pixels } else { None })?;

    // For non-SVG images, apply resize after loading
    let img = if !is_svg(path) {
        if let Some((max_w, max_h)) = max_pixels {
            let (w, h) = img.dimensions();
            if w > max_w || h > max_h {
                tracing::info!(
                    "Resizing image from {}x{} to fit within {}x{}",
                    w,
                    h,
                    max_w,
                    max_h
                );
                img.resize(max_w, max_h, image::imageops::FilterType::Lanczos3)
            } else {
                img
            }
        } else {
            img
        }
    } else {
        img
    };

    let (width, height) = img.dimensions();
    tracing::info!("Processing {}x{} image", width, height);

    let bools: Vec<bool> = match threshold {
        ThresholdMode::Luminance(thresh) => {
            let mut gray = img.to_luma8();
            if dither {
                floyd_steinberg_dither(&mut gray, thresh);
            }
            gray.pixels().map(|Luma([v])| *v < thresh).collect()
        }
        ThresholdMode::Otsu => {
            let mut gray = img.to_luma8();
            let thresh = otsu_threshold(&gray);
            tracing::info!("Otsu threshold: {}", thresh);
            if dither {
                floyd_steinberg_dither(&mut gray, thresh);
            }
            gray.pixels().map(|Luma([v])| *v < thresh).collect()
        }
        ThresholdMode::Alpha(thresh) => {
            if dither {
                // Dither the alpha channel
                let rgba = img.to_rgba8();
                let mut alpha_gray = image::GrayImage::from_raw(
                    width,
                    height,
                    rgba.pixels().map(|p| p[3]).collect(),
                )
                .expect("alpha buffer size matches");
                floyd_steinberg_dither(&mut alpha_gray, thresh);
                alpha_gray.pixels().map(|Luma([v])| *v >= thresh).collect()
            } else {
                let rgba = img.to_rgba8();
                rgba.pixels().map(|p| p[3] >= thresh).collect()
            }
        }
    };

    Ok(ArtworkBitmap::from_bools(width, height, &bools))
}

/// Otsu's method for automatic threshold selection
fn otsu_threshold(img: &image::GrayImage) -> u8 {
    let mut histogram = [0u64; 256];
    for pixel in img.pixels() {
        histogram[pixel[0] as usize] += 1;
    }

    let total = img.width() as f64 * img.height() as f64;
    let sum_total: f64 = histogram
        .iter()
        .enumerate()
        .map(|(i, &count)| i as f64 * count as f64)
        .sum();

    let mut sum_bg = 0.0;
    let mut weight_bg = 0.0;
    let mut max_variance = 0.0;
    let mut best_threshold = 0u8;

    for (t, &count) in histogram.iter().enumerate() {
        weight_bg += count as f64;
        if weight_bg == 0.0 {
            continue;
        }
        let weight_fg = total - weight_bg;
        if weight_fg == 0.0 {
            break;
        }

        sum_bg += t as f64 * count as f64;
        let mean_bg = sum_bg / weight_bg;
        let mean_fg = (sum_total - sum_bg) / weight_fg;

        let variance = weight_bg * weight_fg * (mean_bg - mean_fg).powi(2);
        if variance > max_variance {
            max_variance = variance;
            best_threshold = t as u8;
        }
    }

    best_threshold
}

/// Clear bits [start, start+len) in a bitset using word-level operations.
/// Mirrors `bulk_set_bits` in polygon.rs but uses `&= !mask` instead of `|= mask`.
#[inline]
fn bulk_clear_bits(words: &mut [u64], start: usize, len: usize) {
    if len == 0 {
        return;
    }
    let end = start + len;
    let first_word = start / 64;
    let last_word = (end - 1) / 64;
    let first_bit = start % 64;
    let last_bit_incl = (end - 1) % 64;

    if first_word == last_word {
        let mask = (if last_bit_incl == 63 {
            !0u64
        } else {
            (1u64 << (last_bit_incl + 1)) - 1
        }) & (!0u64 << first_bit);
        words[first_word] &= !mask;
        return;
    }

    words[first_word] &= !(!0u64 << first_bit);
    for w in &mut words[first_word + 1..last_word] {
        *w = 0;
    }
    words[last_word] &= !(if last_bit_incl == 63 {
        !0u64
    } else {
        (1u64 << (last_bit_incl + 1)) - 1
    });
}

/// Count set bits in [start, start+len) using word-level popcount.
#[inline]
fn count_bits_in_range(words: &[u64], start: usize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let end = start + len;
    let first_word = start / 64;
    let last_word = (end - 1) / 64;
    let first_bit = start % 64;
    let last_bit_incl = (end - 1) % 64;

    if first_word == last_word {
        let mask = (if last_bit_incl == 63 {
            !0u64
        } else {
            (1u64 << (last_bit_incl + 1)) - 1
        }) & (!0u64 << first_bit);
        return (words[first_word] & mask).count_ones() as usize;
    }

    let mut count = (words[first_word] & (!0u64 << first_bit)).count_ones() as usize;
    for w in &words[first_word + 1..last_word] {
        count += w.count_ones() as usize;
    }
    let last_mask = if last_bit_incl == 63 {
        !0u64
    } else {
        (1u64 << (last_bit_incl + 1)) - 1
    };
    count += (words[last_word] & last_mask).count_ones() as usize;
    count
}

/// Apply exclusion zones to the bitmap, masking out pixels that overlap with
/// existing metal rectangles (grown by margin). Used to avoid placing artwork
/// over bond pads, power straps, seal ring, etc.
///
/// Uses scan-line rasterization: for each exclusion rect, compute the pixel
/// column/row range via inverse coordinate mapping, then bulk-clear those bits.
/// O(total_exclusion_area_in_pixels) instead of O(pixels * log(rects)).
///
/// The mapping from bitmap pixels to physical coordinates:
/// pixel (x, y) covers physical range [x * pitch, x * pitch + pixel_w] x
/// [(H-1-y) * pitch, (H-1-y) * pitch + pixel_w] in dbu.
pub fn apply_exclusion_mask(
    bitmap: &mut ArtworkBitmap,
    exclusion_rects: &[crate::polygon::Rect],
    pdk: &PdkConfig,
    margin_dbu: i32,
) {
    let min_w_um = pdk.snap_to_grid(pdk.drc.min_width);
    let min_s_um = pdk.snap_to_grid(pdk.drc.min_spacing);
    let pitch_dbu = pdk.um_to_dbu(min_w_um + min_s_um);
    let pixel_w_dbu = pdk.um_to_dbu(min_w_um);

    if pitch_dbu <= 0 || pixel_w_dbu <= 0 {
        return;
    }

    let bw = bitmap.width as usize;
    let bh = bitmap.height as i32;

    let mut masked = 0usize;

    for er in exclusion_rects {
        // Grow by margin
        let gx0 = er.x0 - margin_dbu;
        let gy0 = er.y0 - margin_dbu;
        let gx1 = er.x1 + margin_dbu;
        let gy1 = er.y1 + margin_dbu;

        // Inverse mapping: pixel x overlaps grown rect if
        //   x * pitch < gx1  AND  x * pitch + pixel_w > gx0
        // => x < gx1 / pitch  AND  x >= ceil((gx0 - pixel_w + 1) / pitch)
        // ceil(a / b) for non-negative a, positive b
        let x_start = {
            let a = (gx0 - pixel_w_dbu + 1).max(0);
            ((a + pitch_dbu - 1) / pitch_dbu).max(0) as usize
        };
        let x_end = if gx1 <= 0 {
            0usize
        } else {
            // x < gx1 / pitch, but also x * pitch + pixel_w > gx0, so:
            // last valid x: floor((gx1 - 1) / pitch), but only if x*pitch < gx1
            (((gx1 - 1) / pitch_dbu + 1) as usize).min(bw)
        };

        if x_start >= x_end {
            continue;
        }

        // Inverse mapping for y: pixel y maps to physical_y = (H-1-y) * pitch
        // pixel overlaps if (H-1-y)*pitch < gy1 AND (H-1-y)*pitch + pixel_w > gy0
        // => H-1-y < gy1/pitch AND H-1-y >= ceil((gy0-pixel_w+1)/pitch)
        // Let j = H-1-y, so y = H-1-j
        let j_start = {
            let a = (gy0 - pixel_w_dbu + 1).max(0);
            (a + pitch_dbu - 1) / pitch_dbu
        };
        let j_end = if gy1 <= 0 {
            0
        } else {
            ((gy1 - 1) / pitch_dbu + 1).min(bh)
        };

        if j_start >= j_end {
            continue;
        }

        let col_len = x_end - x_start;
        let words = bitmap.words_mut();

        for j in j_start..j_end {
            let y = (bh - 1 - j) as usize;
            let row_bit_start = y * bw + x_start;
            masked += count_bits_in_range(words, row_bit_start, col_len);
            bulk_clear_bits(words, row_bit_start, col_len);
        }
    }

    tracing::info!(
        "Exclusion mask: cleared {} pixels ({} exclusion rects, margin {} dbu)",
        masked,
        exclusion_rects.len(),
        margin_dbu
    );
}

/// Enforce maximum pixel density by clearing interior pixels in dense windows.
///
/// Uses a summed-area table (SAT) for O(1) window density queries. Iterates until
/// all windows satisfy `density_max`, removing interior pixels (high neighbor count)
/// first to preserve thin features and edges.
///
/// The window step is `window_pixels / 2`, matching `drc::check_density`.
///
/// Returns the total number of pixels cleared.
pub fn enforce_density(bitmap: &mut ArtworkBitmap, density_max: f64, window_pixels: u32) -> usize {
    if density_max >= 1.0 || window_pixels == 0 {
        return 0;
    }

    let w = bitmap.width;
    let h = bitmap.height;
    let win = window_pixels;
    let window_area = (win * win) as f64;
    let max_count = (density_max * window_area).floor() as u32;
    let step = (win / 2).max(1);

    let mut total_cleared = 0usize;
    let max_iters = 20u32;

    // Pre-allocate SAT buffer and reuse across iterations
    let stride = (w + 1) as usize;
    let mut sat = vec![0u32; stride * (h + 1) as usize];
    let mut min_dirty_y = 0u32; // First row that needs SAT rebuild

    for _iter in 0..max_iters {
        // Incremental SAT rebuild: only recompute from min_dirty_y
        build_sat_from(bitmap, w, h, &mut sat, min_dirty_y);

        // Find violating windows, sorted worst-first
        let mut violations: Vec<(u32, u32, u32)> = Vec::new(); // (count, wx, wy)
        let mut wx = 0u32;
        while wx + win <= w {
            let mut wy = 0u32;
            while wy + win <= h {
                let count = sat_query(&sat, w, wx, wy, wx + win, wy + win);
                if count > max_count {
                    violations.push((count, wx, wy));
                }
                wy += step;
            }
            wx += step;
        }

        if violations.is_empty() {
            break;
        }

        // Sort worst-first (highest density)
        violations.sort_unstable_by(|a, b| b.0.cmp(&a.0));

        let mut cleared_this_iter = 0usize;
        min_dirty_y = h; // Track earliest modified row for next iteration

        for &(_count, vx, vy) in &violations {
            // Recount since previous removals in this iteration may have helped
            let current = count_on_in_window(bitmap, vx, vy, win, win);
            if current <= max_count {
                continue;
            }
            let excess = current - max_count;

            // Collect on-pixels in window, scored by neighbor count (8-connected)
            let mut candidates: Vec<(u8, u32, u32)> = Vec::new();
            for py in vy..vy + win {
                for px in vx..vx + win {
                    if bitmap.get(px, py) {
                        let nb = count_neighbors(bitmap, px, py);
                        candidates.push((nb, px, py));
                    }
                }
            }

            // Sort by removability: highest neighbor count first (interior pixels)
            // Only remove pixels with >= 3 neighbors to preserve thin lines
            candidates.sort_unstable_by(|a, b| b.0.cmp(&a.0));

            let mut removed = 0u32;
            for &(nb, px, py) in &candidates {
                if removed >= excess {
                    break;
                }
                if nb < 3 {
                    break; // Don't remove edge/tip pixels
                }
                if bitmap.get(px, py) {
                    bitmap.set(px, py, false);
                    removed += 1;
                    cleared_this_iter += 1;
                    min_dirty_y = min_dirty_y.min(py);
                }
            }
        }

        total_cleared += cleared_this_iter;
        if cleared_this_iter == 0 {
            break; // No progress possible (all remaining pixels have < 3 neighbors)
        }
    }

    total_cleared
}

/// Build a summed-area table over the bitmap. SAT has dimensions (w+1) x (h+1).
/// Uses word-level iteration over packed u64 words to avoid per-pixel bounds checks.
pub fn build_sat(bitmap: &ArtworkBitmap, w: u32, h: u32) -> Vec<u32> {
    let stride = (w + 1) as usize;
    let mut sat = vec![0u32; stride * (h + 1) as usize];
    build_sat_from(bitmap, w, h, &mut sat, 0);
    sat
}

/// Build SAT rows starting from `start_y`, reusing the provided buffer.
/// Rows before `start_y` are assumed to already be correct in `sat`.
fn build_sat_from(bitmap: &ArtworkBitmap, w: u32, h: u32, sat: &mut [u32], start_y: u32) {
    let stride = (w + 1) as usize;
    let w_usize = w as usize;

    for y in start_y..h {
        let (row_words, bit_offset) = bitmap.row_words(y);
        let sat_row = (y + 1) as usize * stride;
        let sat_prev_row = y as usize * stride;
        let mut row_sum = 0u32;
        let mut x = 0usize;

        // Process bits using word-level extraction
        let mut word_idx = 0usize;
        let mut bit_in_word = bit_offset;

        while x < w_usize {
            let word = row_words[word_idx];
            // How many pixels can we extract from this word?
            let bits_left_in_word = 64 - bit_in_word;
            let pixels_left = w_usize - x;
            let count = bits_left_in_word.min(pixels_left);

            // Extract `count` bits starting at `bit_in_word`
            let shifted = word >> bit_in_word;
            for b in 0..count {
                row_sum += ((shifted >> b) & 1) as u32;
                sat[sat_row + x + b + 1] = row_sum + sat[sat_prev_row + x + b + 1];
            }

            x += count;
            word_idx += 1;
            bit_in_word = 0;
        }
    }
}

/// Query the SAT for the count of on-pixels in [x0, x1) x [y0, y1).
fn sat_query(sat: &[u32], w: u32, x0: u32, y0: u32, x1: u32, y1: u32) -> u32 {
    let stride = (w + 1) as usize;
    let a = sat[y1 as usize * stride + x1 as usize];
    let b = sat[y0 as usize * stride + x1 as usize];
    let c = sat[y1 as usize * stride + x0 as usize];
    let d = sat[y0 as usize * stride + x0 as usize];
    // Use wrapping arithmetic to avoid underflow: (a - b - c + d) = (a + d) - (b + c)
    a.wrapping_sub(b).wrapping_sub(c).wrapping_add(d)
}

/// Direct count of on-pixels in a rectangular window (used for recount after partial removals).
/// Uses word-level iteration to avoid per-pixel bounds checks and division.
pub fn count_on_in_window(bitmap: &ArtworkBitmap, wx: u32, wy: u32, win_w: u32, win_h: u32) -> u32 {
    let mut count = 0u32;
    let bw = bitmap.width as usize;
    let words = bitmap.words();

    for y in wy..wy + win_h {
        let row_bit_start = y as usize * bw + wx as usize;
        let row_bit_end = row_bit_start + win_w as usize;

        let first_word = row_bit_start / 64;
        let last_word = (row_bit_end - 1) / 64;
        let first_bit = row_bit_start % 64;
        let last_bit_incl = (row_bit_end - 1) % 64;

        if first_word == last_word {
            // All bits in one word
            let mask = (if last_bit_incl == 63 {
                !0u64
            } else {
                (1u64 << (last_bit_incl + 1)) - 1
            }) & (!0u64 << first_bit);
            count += (words[first_word] & mask).count_ones();
        } else {
            // First partial word
            count += (words[first_word] & (!0u64 << first_bit)).count_ones();
            // Full interior words
            for w in &words[first_word + 1..last_word] {
                count += w.count_ones();
            }
            // Last partial word
            let last_mask = if last_bit_incl == 63 {
                !0u64
            } else {
                (1u64 << (last_bit_incl + 1)) - 1
            };
            count += (words[last_word] & last_mask).count_ones();
        }
    }
    count
}

/// Count 8-connected on-neighbors of pixel (x, y).
/// Uses direct word access to avoid per-neighbor bounds checks and division.
#[inline]
fn count_neighbors(bitmap: &ArtworkBitmap, x: u32, y: u32) -> u8 {
    let w = bitmap.width as usize;
    let h = bitmap.height;
    let words = bitmap.words();
    let mut count = 0u8;

    let has_left = x > 0;
    let has_right = x + 1 < bitmap.width;
    let has_up = y > 0;
    let has_down = y + 1 < h;

    // Row above
    if has_up {
        let base = (y as usize - 1) * w + x as usize;
        if has_left {
            count += ((words[(base - 1) / 64] >> ((base - 1) % 64)) & 1) as u8;
        }
        count += ((words[base / 64] >> (base % 64)) & 1) as u8;
        if has_right {
            count += ((words[(base + 1) / 64] >> ((base + 1) % 64)) & 1) as u8;
        }
    }

    // Same row
    {
        let base = y as usize * w + x as usize;
        if has_left {
            count += ((words[(base - 1) / 64] >> ((base - 1) % 64)) & 1) as u8;
        }
        if has_right {
            count += ((words[(base + 1) / 64] >> ((base + 1) % 64)) & 1) as u8;
        }
    }

    // Row below
    if has_down {
        let base = (y as usize + 1) * w + x as usize;
        if has_left {
            count += ((words[(base - 1) / 64] >> ((base - 1) % 64)) & 1) as u8;
        }
        count += ((words[base / 64] >> (base % 64)) & 1) as u8;
        if has_right {
            count += ((words[(base + 1) / 64] >> ((base + 1) % 64)) & 1) as u8;
        }
    }

    count
}

/// Enforce maximum pixel density in a specific region of the bitmap.
///
/// This is a targeted version of `enforce_density` that only scans and fixes
/// one rectangular region - much cheaper than a full bitmap pass. Used by the
/// density feedback loop to fix specific DRC-violating windows.
///
/// Returns the number of pixels cleared.
pub fn enforce_density_region(
    bitmap: &mut ArtworkBitmap,
    density_max: f64,
    region_x: u32,
    region_y: u32,
    region_w: u32,
    region_h: u32,
) -> usize {
    if density_max >= 1.0 || region_w == 0 || region_h == 0 {
        return 0;
    }

    // Clamp region to bitmap bounds
    let rx1 = (region_x + region_w).min(bitmap.width);
    let ry1 = (region_y + region_h).min(bitmap.height);
    let rx0 = region_x.min(rx1);
    let ry0 = region_y.min(ry1);
    let rw = rx1 - rx0;
    let rh = ry1 - ry0;
    if rw == 0 || rh == 0 {
        return 0;
    }

    let window_area = (rw * rh) as f64;
    let max_count = (density_max * window_area).floor() as u32;

    let mut total_cleared = 0usize;

    for _iter in 0..10 {
        // Count current on-pixels in the region
        let current = count_on_in_window(bitmap, rx0, ry0, rw, rh);
        if current <= max_count {
            break;
        }
        let excess = current - max_count;

        // Collect on-pixels scored by neighbor count (interior first)
        let mut candidates: Vec<(u8, u32, u32)> = Vec::new();
        for py in ry0..ry1 {
            for px in rx0..rx1 {
                if bitmap.get(px, py) {
                    let nb = count_neighbors(bitmap, px, py);
                    candidates.push((nb, px, py));
                }
            }
        }

        // Sort: highest neighbor count first (interior pixels removed first)
        candidates.sort_unstable_by(|a, b| b.0.cmp(&a.0));

        let mut removed = 0u32;
        for &(nb, px, py) in &candidates {
            if removed >= excess {
                break;
            }
            if nb < 3 {
                break; // Don't remove edge/tip pixels
            }
            if bitmap.get(px, py) {
                bitmap.set(px, py, false);
                removed += 1;
                total_cleared += 1;
            }
        }

        if removed == 0 {
            break; // No progress possible
        }
    }

    total_cleared
}

/// Resize the bitmap to fit within target dimensions while preserving aspect ratio
pub fn resize_bitmap(bmp: &ArtworkBitmap, max_w: u32, max_h: u32) -> ArtworkBitmap {
    let scale_x = max_w as f64 / bmp.width as f64;
    let scale_y = max_h as f64 / bmp.height as f64;
    let scale = scale_x.min(scale_y).min(1.0);

    let new_w = (bmp.width as f64 * scale).round() as u32;
    let new_h = (bmp.height as f64 * scale).round() as u32;

    let mut result = ArtworkBitmap::new_zeroed(new_w, new_h);
    for y in 0..new_h {
        for x in 0..new_w {
            let src_x = (x as f64 / scale).min((bmp.width - 1) as f64) as u32;
            let src_y = (y as f64 / scale).min((bmp.height - 1) as f64) as u32;
            if bmp.get(src_x, src_y) {
                result.set(x, y, true);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitmap_basics() {
        let bmp = ArtworkBitmap::from_bools(
            4,
            4,
            &[
                true, true, false, false, true, true, false, false, false, false, true, true,
                false, false, true, true,
            ],
        );
        assert_eq!(bmp.metal_count(), 8);
        assert!((bmp.density() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_bitset_roundtrip() {
        // Non-multiple-of-64 width
        let width = 7u32;
        let height = 5u32;
        let bools: Vec<bool> = (0..width * height).map(|i| i % 3 == 0).collect();
        let expected_count = bools.iter().filter(|&&b| b).count();

        let bmp = ArtworkBitmap::from_bools(width, height, &bools);
        assert_eq!(bmp.metal_count(), expected_count);

        // Verify every pixel round-trips
        for y in 0..height {
            for x in 0..width {
                let expected = bools[(y * width + x) as usize];
                assert_eq!(bmp.get(x, y), expected, "mismatch at ({x}, {y})");
            }
        }

        // Test set/clear
        let mut bmp2 = bmp.clone();
        bmp2.set(0, 0, false);
        assert!(!bmp2.get(0, 0));
        bmp2.set(0, 0, true);
        assert!(bmp2.get(0, 0));

        // Test invert
        let mut bmp3 = bmp.clone();
        bmp3.invert();
        assert_eq!(
            bmp3.metal_count(),
            (width * height) as usize - expected_count
        );
        for y in 0..height {
            for x in 0..width {
                assert_eq!(bmp3.get(x, y), !bmp.get(x, y));
            }
        }
    }

    #[test]
    fn test_bitset_exact_64_width() {
        let bmp = ArtworkBitmap::from_bools(64, 1, &[true; 64]);
        assert_eq!(bmp.metal_count(), 64);
    }

    #[test]
    fn test_exclusion_mask() {
        use crate::polygon::Rect;

        let pdk = crate::pdk::PdkConfig::builtin("sky130").unwrap();
        let min_w_um = pdk.snap_to_grid(pdk.drc.min_width);
        let min_s_um = pdk.snap_to_grid(pdk.drc.min_spacing);
        let pitch = pdk.um_to_dbu(min_w_um + min_s_um);
        let pw = pdk.um_to_dbu(min_w_um);

        // 4x4 bitmap, all on
        let mut bmp = ArtworkBitmap::from_bools(4, 4, &[true; 16]);

        // Exclusion rect covering pixel (1, 0) in bitmap coords
        // Pixel (1, 0) maps to physical: x=[1*pitch, 1*pitch+pw], y=[(4-1-0)*pitch, 3*pitch+pw]
        let excl = vec![Rect::new(pitch, 3 * pitch, pitch + pw, 3 * pitch + pw)];

        apply_exclusion_mask(&mut bmp, &excl, &pdk, 0);
        assert!(!bmp.get(1, 0), "Pixel (1,0) should be masked");
        assert!(bmp.get(0, 0), "Pixel (0,0) should remain");
        assert_eq!(bmp.metal_count(), 15);
    }

    #[test]
    fn test_exclusion_mask_with_margin() {
        use crate::polygon::Rect;

        let pdk = crate::pdk::PdkConfig::builtin("sky130").unwrap();
        let min_w_um = pdk.snap_to_grid(pdk.drc.min_width);
        let min_s_um = pdk.snap_to_grid(pdk.drc.min_spacing);
        let pitch = pdk.um_to_dbu(min_w_um + min_s_um);
        let pw = pdk.um_to_dbu(min_w_um);

        let mut bmp = ArtworkBitmap::from_bools(4, 4, &[true; 16]);

        // Small exclusion rect; with large margin it should catch neighbors
        let excl = vec![Rect::new(
            pitch + pw / 2,
            3 * pitch + pw / 2,
            pitch + pw / 2 + 1,
            3 * pitch + pw / 2 + 1,
        )];

        // Large margin should mask pixel (1,0) and possibly neighbors
        apply_exclusion_mask(&mut bmp, &excl, &pdk, pitch);
        assert!(!bmp.get(1, 0), "Pixel (1,0) should be masked with margin");
        assert!(bmp.metal_count() < 16, "Some pixels should be masked");
    }

    #[test]
    fn test_enforce_density_noop_below_threshold() {
        // Sparse bitmap should not be modified
        let mut bmp = ArtworkBitmap::from_bools(10, 10, &{
            let mut v = vec![false; 100];
            // 10 on-pixels out of 100 = 10% density
            for i in (0..100).step_by(10) {
                v[i] = true;
            }
            v
        });
        let original_count = bmp.metal_count();
        let cleared = enforce_density(&mut bmp, 0.80, 10);
        assert_eq!(cleared, 0);
        assert_eq!(bmp.metal_count(), original_count);
    }

    #[test]
    fn test_enforce_density_solid_thinned() {
        // Fully solid 20x20 bitmap (100% density) should be thinned to <= 80%
        let mut bmp = ArtworkBitmap::from_bools(20, 20, &vec![true; 400]);
        let cleared = enforce_density(&mut bmp, 0.80, 20);
        assert!(cleared > 0, "Should have cleared some pixels");
        // Check density in the full window
        let count = bmp.metal_count();
        let density = count as f64 / 400.0;
        assert!(
            density <= 0.80 + 0.01,
            "Density {:.2} should be <= 0.80",
            density
        );
    }

    #[test]
    fn test_enforce_density_preserves_edges() {
        // A thin 1-pixel-wide line should not be destroyed (neighbors < 3)
        let mut bmp = ArtworkBitmap::new_zeroed(20, 20);
        // Horizontal line at row 10
        for x in 0..20 {
            bmp.set(x, 10, true);
        }
        let original_count = bmp.metal_count();
        let cleared = enforce_density(&mut bmp, 0.10, 20);
        // The line pixels have at most 2 neighbors (left/right), so none should be removed
        assert_eq!(cleared, 0);
        assert_eq!(bmp.metal_count(), original_count);
    }

    #[test]
    fn test_enforce_density_multiple_windows() {
        // Two dense blocks separated by gap, both should be thinned
        let mut bmp = ArtworkBitmap::new_zeroed(30, 10);
        // Block 1: columns 0-9 fully on
        for y in 0..10 {
            for x in 0..10 {
                bmp.set(x, y, true);
            }
        }
        // Block 2: columns 20-29 fully on
        for y in 0..10 {
            for x in 20..30 {
                bmp.set(x, y, true);
            }
        }
        let cleared = enforce_density(&mut bmp, 0.70, 10);
        assert!(cleared > 0, "Should thin dense blocks");
    }

    #[test]
    fn test_enforce_density_converges() {
        // Large solid bitmap should converge within iteration cap
        let mut bmp = ArtworkBitmap::from_bools(50, 50, &vec![true; 2500]);
        let cleared = enforce_density(&mut bmp, 0.75, 25);
        assert!(cleared > 0);
        // Verify all windows now satisfy the constraint
        let win = 25u32;
        let step = win / 2;
        let mut wx = 0u32;
        while wx + win <= 50 {
            let mut wy = 0u32;
            while wy + win <= 50 {
                let mut count = 0u32;
                for y in wy..wy + win {
                    for x in wx..wx + win {
                        count += bmp.get(x, y) as u32;
                    }
                }
                let density = count as f64 / (win * win) as f64;
                assert!(
                    density <= 0.75 + 0.02,
                    "Window ({},{}) density {:.3} exceeds limit",
                    wx,
                    wy,
                    density
                );
                wy += step;
            }
            wx += step;
        }
    }

    #[test]
    fn test_enforce_density_region_thins_target_only() {
        // Create bitmap with two dense blocks: one at top-left, one at bottom-right
        let mut bmp = ArtworkBitmap::new_zeroed(30, 30);
        // Block 1: (0,0)-(9,9) fully solid
        for y in 0..10u32 {
            for x in 0..10u32 {
                bmp.set(x, y, true);
            }
        }
        // Block 2: (20,20)-(29,29) fully solid
        for y in 20..30u32 {
            for x in 20..30u32 {
                bmp.set(x, y, true);
            }
        }

        // Enforce density only on block 1's region
        let cleared = enforce_density_region(&mut bmp, 0.70, 0, 0, 10, 10);
        assert!(cleared > 0, "Should thin the targeted region");

        // Block 1 should have reduced density
        let mut block1_count = 0u32;
        for y in 0..10u32 {
            for x in 0..10u32 {
                block1_count += bmp.get(x, y) as u32;
            }
        }
        assert!(
            block1_count <= 70,
            "Block 1 density should be <= 70%, got {}%",
            block1_count
        );

        // Block 2 should be untouched (still 100 pixels)
        let mut block2_count = 0u32;
        for y in 20..30u32 {
            for x in 20..30u32 {
                block2_count += bmp.get(x, y) as u32;
            }
        }
        assert_eq!(block2_count, 100, "Block 2 should be unaffected");
    }

    #[test]
    fn test_enforce_density_region_noop_sparse() {
        // Sparse region should not be modified
        let mut bmp = ArtworkBitmap::new_zeroed(20, 20);
        // Just a few scattered pixels
        bmp.set(5, 5, true);
        bmp.set(10, 10, true);
        bmp.set(15, 15, true);
        let cleared = enforce_density_region(&mut bmp, 0.80, 0, 0, 20, 20);
        assert_eq!(cleared, 0);
        assert_eq!(bmp.metal_count(), 3);
    }

    #[test]
    fn test_otsu_bimodal() {
        // Create a simple bimodal image
        let mut img = image::GrayImage::new(100, 100);
        for y in 0..100 {
            for x in 0..100 {
                let v = if x < 50 { 30 } else { 220 };
                img.put_pixel(x, y, Luma([v]));
            }
        }
        let thresh = otsu_threshold(&img);
        assert!(
            (30..220).contains(&thresh),
            "Otsu threshold {} should be between 30 and 220",
            thresh
        );
    }

    #[test]
    fn test_floyd_steinberg_gradient_density() {
        // A gradient image should dither to roughly 50% density
        let mut gray = image::GrayImage::new(100, 100);
        for y in 0..100 {
            for x in 0..100 {
                let v = ((x as f32 / 99.0) * 255.0) as u8;
                gray.put_pixel(x, y, Luma([v]));
            }
        }
        floyd_steinberg_dither(&mut gray, 128);
        let on: usize = gray.pixels().filter(|Luma([v])| *v < 128).count();
        let density = on as f64 / 10000.0;
        assert!(
            (0.35..0.65).contains(&density),
            "Gradient dither density {:.2} should be near 50%",
            density
        );
    }

    #[test]
    fn test_floyd_steinberg_preserves_extremes() {
        // Pure black stays black, pure white stays white
        let mut black = image::GrayImage::from_pixel(10, 10, Luma([0]));
        floyd_steinberg_dither(&mut black, 128);
        assert!(black.pixels().all(|Luma([v])| *v == 0));

        let mut white = image::GrayImage::from_pixel(10, 10, Luma([255]));
        floyd_steinberg_dither(&mut white, 128);
        assert!(white.pixels().all(|Luma([v])| *v == 255));
    }

    #[test]
    fn test_is_svg() {
        assert!(is_svg(Path::new("logo.svg")));
        assert!(is_svg(Path::new("logo.SVG")));
        assert!(is_svg(Path::new("logo.svgz")));
        assert!(!is_svg(Path::new("logo.png")));
        assert!(!is_svg(Path::new("logo.jpg")));
    }

    #[test]
    fn test_rasterize_svg_simple() {
        // Create a simple SVG with a black rectangle
        let svg_content = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
            <rect x="0" y="0" width="100" height="100" fill="black"/>
        </svg>"#;
        let tmp = tempfile::NamedTempFile::with_suffix(".svg").unwrap();
        std::fs::write(tmp.path(), svg_content).unwrap();

        let img = rasterize_svg(tmp.path(), None).unwrap();
        let (w, h) = img.dimensions();
        assert_eq!(w, 100);
        assert_eq!(h, 100);

        // Check that pixels are dark (black rect)
        let gray = img.to_luma8();
        let avg: f64 = gray.pixels().map(|Luma([v])| *v as f64).sum::<f64>() / (w * h) as f64;
        assert!(
            avg < 10.0,
            "Black SVG should produce dark pixels, got avg {}",
            avg
        );
    }

    #[test]
    fn test_rasterize_svg_with_resize() {
        let svg_content = r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
            <rect x="0" y="0" width="200" height="100" fill="red"/>
        </svg>"#;
        let tmp = tempfile::NamedTempFile::with_suffix(".svg").unwrap();
        std::fs::write(tmp.path(), svg_content).unwrap();

        let img = rasterize_svg(tmp.path(), Some((50, 50))).unwrap();
        let (w, h) = img.dimensions();
        // Should fit within 50x50, preserving 2:1 aspect ratio
        assert!(w <= 50 && h <= 50);
        assert_eq!(w, 50); // Width-limited
        assert_eq!(h, 25); // Half height due to aspect ratio
    }
}
