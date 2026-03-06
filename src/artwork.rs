use crate::pdk::PdkConfig;
use anyhow::{Context, Result};
use image::{GenericImageView, Luma};
use std::path::Path;

/// A binary bitmap where true = metal, false = gap
#[derive(Debug, Clone)]
pub struct ArtworkBitmap {
    pub width: u32,
    pub height: u32,
    /// Row-major: pixels[y * width + x]
    pub pixels: Vec<bool>,
}

impl ArtworkBitmap {
    pub fn get(&self, x: u32, y: u32) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        self.pixels[(y * self.width + x) as usize]
    }

    pub fn set(&mut self, x: u32, y: u32, val: bool) {
        if x < self.width && y < self.height {
            self.pixels[(y * self.width + x) as usize] = val;
        }
    }

    /// Count of "on" pixels
    pub fn metal_count(&self) -> usize {
        self.pixels.iter().filter(|&&p| p).count()
    }

    /// Metal density as a fraction
    pub fn density(&self) -> f64 {
        self.metal_count() as f64 / (self.width * self.height) as f64
    }

    /// Invert the bitmap (swap metal/gap)
    pub fn invert(&mut self) {
        for p in &mut self.pixels {
            *p = !*p;
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

/// Load an image file and convert to a binary artwork bitmap.
///
/// If `max_pixels` is set, the image is downscaled so that the
/// total pixel count for the artwork doesn't exceed it (useful for
/// keeping GDSII file sizes reasonable).
pub fn load_artwork(
    path: &Path,
    threshold: ThresholdMode,
    max_pixels: Option<(u32, u32)>,
) -> Result<ArtworkBitmap> {
    let img =
        image::open(path).with_context(|| format!("Failed to open image: {}", path.display()))?;

    let img = if let Some((max_w, max_h)) = max_pixels {
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
    };

    let (width, height) = img.dimensions();
    tracing::info!("Processing {}x{} image", width, height);

    let pixels = match threshold {
        ThresholdMode::Luminance(thresh) => {
            let gray = img.to_luma8();
            gray.pixels().map(|Luma([v])| *v < thresh).collect()
        }
        ThresholdMode::Otsu => {
            let gray = img.to_luma8();
            let thresh = otsu_threshold(&gray);
            tracing::info!("Otsu threshold: {}", thresh);
            gray.pixels().map(|Luma([v])| *v < thresh).collect()
        }
        ThresholdMode::Alpha(thresh) => {
            let rgba = img.to_rgba8();
            rgba.pixels()
                .map(|p| p[3] >= thresh) // alpha >= threshold = metal
                .collect()
        }
    };

    Ok(ArtworkBitmap {
        width,
        height,
        pixels,
    })
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

/// Apply exclusion zones to the bitmap, masking out pixels that overlap with
/// existing metal rectangles (grown by margin). Used to avoid placing artwork
/// over bond pads, power straps, seal ring, etc.
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

    let mut masked = 0usize;
    for y in 0..bitmap.height {
        let py0 = (bitmap.height - 1 - y) as i32 * pitch_dbu;
        let py1 = py0 + pixel_w_dbu;
        for x in 0..bitmap.width {
            if !bitmap.get(x, y) {
                continue;
            }
            let px0 = x as i32 * pitch_dbu;
            let px1 = px0 + pixel_w_dbu;

            for er in exclusion_rects {
                // Grow exclusion rect by margin
                let ex0 = er.x0 - margin_dbu;
                let ey0 = er.y0 - margin_dbu;
                let ex1 = er.x1 + margin_dbu;
                let ey1 = er.y1 + margin_dbu;

                // Check overlap (ranges intersect)
                if px0 < ex1 && px1 > ex0 && py0 < ey1 && py1 > ey0 {
                    bitmap.set(x, y, false);
                    masked += 1;
                    break;
                }
            }
        }
    }

    tracing::info!(
        "Exclusion mask: cleared {} pixels ({} exclusion rects, margin {} dbu)",
        masked,
        exclusion_rects.len(),
        margin_dbu
    );
}

/// Resize the bitmap to fit within target dimensions while preserving aspect ratio
pub fn resize_bitmap(bmp: &ArtworkBitmap, max_w: u32, max_h: u32) -> ArtworkBitmap {
    let scale_x = max_w as f64 / bmp.width as f64;
    let scale_y = max_h as f64 / bmp.height as f64;
    let scale = scale_x.min(scale_y).min(1.0);

    let new_w = (bmp.width as f64 * scale).round() as u32;
    let new_h = (bmp.height as f64 * scale).round() as u32;

    let mut pixels = vec![false; (new_w * new_h) as usize];
    for y in 0..new_h {
        for x in 0..new_w {
            let src_x = (x as f64 / scale).min((bmp.width - 1) as f64) as u32;
            let src_y = (y as f64 / scale).min((bmp.height - 1) as f64) as u32;
            pixels[(y * new_w + x) as usize] = bmp.get(src_x, src_y);
        }
    }

    ArtworkBitmap {
        width: new_w,
        height: new_h,
        pixels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitmap_basics() {
        let bmp = ArtworkBitmap {
            width: 4,
            height: 4,
            pixels: vec![
                true, true, false, false, true, true, false, false, false, false, true, true,
                false, false, true, true,
            ],
        };
        assert_eq!(bmp.metal_count(), 8);
        assert!((bmp.density() - 0.5).abs() < 1e-6);
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
        let mut bmp = ArtworkBitmap {
            width: 4,
            height: 4,
            pixels: vec![true; 16],
        };

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

        let mut bmp = ArtworkBitmap {
            width: 4,
            height: 4,
            pixels: vec![true; 16],
        };

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
}
