use crate::artwork::{ArtworkBitmap, ThresholdMode};
use crate::pdk::ArtworkLayerProfile;
use anyhow::{Context, Result};
use std::path::Path;

/// Color extraction mode for multi-layer artwork
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    /// Extract R/G/B channels and map to layers by profile color field
    Channel,
    /// K-means quantize into N colors, one per layer
    Palette,
}

/// A bitmap associated with a specific layer profile index
pub struct LayerBitmap {
    pub bitmap: ArtworkBitmap,
    pub layer_index: usize,
}

/// Load image and open it with optional resize, returning the DynamicImage.
fn load_image(path: &Path, max_pixels: Option<(u32, u32)>) -> Result<image::DynamicImage> {
    let img =
        image::open(path).with_context(|| format!("Failed to open image: {}", path.display()))?;
    let img = if let Some((max_w, max_h)) = max_pixels {
        let (w, h) = image::GenericImageView::dimensions(&img);
        if w > max_w || h > max_h {
            img.resize(max_w, max_h, image::imageops::FilterType::Lanczos3)
        } else {
            img
        }
    } else {
        img
    };
    Ok(img)
}

/// Extract color channels (R, G, B) into separate bitmaps.
///
/// Each `ArtworkLayerProfile` with a `color` field matching "red", "green", or "blue"
/// gets its corresponding channel thresholded into a binary bitmap.
pub fn extract_channels(
    path: &Path,
    profiles: &[ArtworkLayerProfile],
    threshold: ThresholdMode,
    max_pixels: Option<(u32, u32)>,
) -> Result<Vec<LayerBitmap>> {
    let img = load_image(path, max_pixels)?;
    let (width, height) = image::GenericImageView::dimensions(&img);
    let rgba = img.to_rgba8();

    let thresh_val = match threshold {
        ThresholdMode::Luminance(v) => v,
        ThresholdMode::Otsu => {
            // Use luminance-based Otsu on the overall image
            let gray = img.to_luma8();
            otsu_threshold_gray(&gray)
        }
        ThresholdMode::Alpha(v) => v,
    };

    let mut results = Vec::new();

    for (i, profile) in profiles.iter().enumerate() {
        let color = match &profile.color {
            Some(c) => c.to_lowercase(),
            None => continue,
        };

        let channel_idx = match color.as_str() {
            "red" | "r" => 0,
            "green" | "g" => 1,
            "blue" | "b" => 2,
            _ => {
                tracing::warn!(
                    "Unknown color '{}' for layer '{}', skipping",
                    color,
                    profile.name
                );
                continue;
            }
        };

        let bools: Vec<bool> = rgba
            .pixels()
            .map(|p| p[channel_idx] >= thresh_val)
            .collect();

        let bitmap = ArtworkBitmap::from_bools(width, height, &bools);
        tracing::info!(
            "Channel '{}' -> layer '{}': {}x{}, density {:.1}%",
            color,
            profile.name,
            width,
            height,
            bitmap.density() * 100.0
        );

        results.push(LayerBitmap {
            bitmap,
            layer_index: i,
        });
    }

    if results.is_empty() {
        anyhow::bail!(
            "No layer profiles have a color field matching R/G/B. \
             Add color = \"red\"/\"green\"/\"blue\" to artwork_layers in your PDK TOML."
        );
    }

    Ok(results)
}

/// Quantize image colors into N+1 clusters via k-means, drop the brightest (background),
/// and return the N darkest clusters as layer bitmaps (darkest first = layer 0).
///
/// This follows the artwork convention: dark = metal, bright = gap/background.
pub fn extract_palette(
    path: &Path,
    num_layers: usize,
    threshold: ThresholdMode,
    max_pixels: Option<(u32, u32)>,
) -> Result<Vec<LayerBitmap>> {
    anyhow::ensure!(num_layers >= 1, "num_layers must be >= 1");

    let img = load_image(path, max_pixels)?;
    let (width, height) = image::GenericImageView::dimensions(&img);
    let rgb = img.to_rgb8();
    let pixels: Vec<[f32; 3]> = rgb
        .pixels()
        .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
        .collect();

    let _ = threshold; // Palette mode uses k-means, not threshold

    // Quantize into num_layers + 1 clusters: the extra one captures background
    let k = num_layers + 1;
    let centroids = kmeans(&pixels, k, 15);

    // Assign each pixel to nearest centroid
    let assignments: Vec<usize> = pixels
        .iter()
        .map(|px| nearest_centroid(px, &centroids))
        .collect();

    // Sort centroids by luminance (darkest first)
    let mut centroid_order: Vec<usize> = (0..k).collect();
    centroid_order.sort_by(|&a, &b| {
        let lum_a = luminance(&centroids[a]);
        let lum_b = luminance(&centroids[b]);
        lum_a
            .partial_cmp(&lum_b)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // The last entry (brightest) is background - skip it
    let bg_cluster = *centroid_order.last().unwrap();
    let bg_c = &centroids[bg_cluster];
    tracing::info!(
        "Palette background: centroid RGB({:.0},{:.0},{:.0}) - skipped",
        bg_c[0],
        bg_c[1],
        bg_c[2],
    );

    // Build reverse mapping for the remaining clusters (darkest first)
    let artwork_clusters: Vec<usize> = centroid_order[..num_layers].to_vec();
    let mut cluster_to_layer: std::collections::HashMap<usize, usize> =
        std::collections::HashMap::new();
    for (layer_idx, &cluster_idx) in artwork_clusters.iter().enumerate() {
        cluster_to_layer.insert(cluster_idx, layer_idx);
    }

    // Create one bitmap per artwork layer
    let mut results = Vec::with_capacity(num_layers);
    for (layer_idx, &orig_cluster) in artwork_clusters.iter().enumerate() {
        let bools: Vec<bool> = assignments.iter().map(|&a| a == orig_cluster).collect();
        let bitmap = ArtworkBitmap::from_bools(width, height, &bools);

        let c = &centroids[orig_cluster];
        tracing::info!(
            "Palette layer {}: centroid RGB({:.0},{:.0},{:.0}), density {:.1}%",
            layer_idx,
            c[0],
            c[1],
            c[2],
            bitmap.density() * 100.0
        );

        results.push(LayerBitmap {
            bitmap,
            layer_index: layer_idx,
        });
    }

    Ok(results)
}

/// Simple k-means clustering on RGB values.
fn kmeans(pixels: &[[f32; 3]], k: usize, max_iters: usize) -> Vec<[f32; 3]> {
    let n = pixels.len();
    if n == 0 || k == 0 {
        return vec![[0.0; 3]; k];
    }

    // Initialize centroids using k-means++ style: pick first as darkest pixel,
    // then each subsequent centroid is the pixel farthest from existing centroids.
    // This avoids the collapse problem when evenly-spaced pixels are similar.
    let mut centroids: Vec<[f32; 3]> = Vec::with_capacity(k);

    // First centroid: darkest pixel
    let first = pixels
        .iter()
        .min_by(|a, b| luminance(a).partial_cmp(&luminance(b)).unwrap())
        .unwrap();
    centroids.push(*first);

    for _ in 1..k {
        // Pick pixel with max distance to nearest existing centroid
        let best = pixels
            .iter()
            .max_by(|a, b| {
                let da: f32 = centroids
                    .iter()
                    .map(|c| (a[0] - c[0]).powi(2) + (a[1] - c[1]).powi(2) + (a[2] - c[2]).powi(2))
                    .fold(f32::MAX, f32::min);
                let db: f32 = centroids
                    .iter()
                    .map(|c| (b[0] - c[0]).powi(2) + (b[1] - c[1]).powi(2) + (b[2] - c[2]).powi(2))
                    .fold(f32::MAX, f32::min);
                da.partial_cmp(&db).unwrap()
            })
            .unwrap();
        centroids.push(*best);
    }

    let mut assignments = vec![0usize; n];

    for _ in 0..max_iters {
        // Assign pixels to nearest centroid
        let mut changed = false;
        for (i, px) in pixels.iter().enumerate() {
            let nearest = nearest_centroid(px, &centroids);
            if nearest != assignments[i] {
                assignments[i] = nearest;
                changed = true;
            }
        }

        if !changed {
            break;
        }

        // Update centroids
        let mut sums = vec![[0.0f64; 3]; k];
        let mut counts = vec![0u64; k];
        for (i, px) in pixels.iter().enumerate() {
            let c = assignments[i];
            sums[c][0] += px[0] as f64;
            sums[c][1] += px[1] as f64;
            sums[c][2] += px[2] as f64;
            counts[c] += 1;
        }

        for (j, centroid) in centroids.iter_mut().enumerate() {
            if counts[j] > 0 {
                centroid[0] = (sums[j][0] / counts[j] as f64) as f32;
                centroid[1] = (sums[j][1] / counts[j] as f64) as f32;
                centroid[2] = (sums[j][2] / counts[j] as f64) as f32;
            }
        }
    }

    centroids
}

#[inline]
fn nearest_centroid(px: &[f32; 3], centroids: &[[f32; 3]]) -> usize {
    let mut best = 0;
    let mut best_dist = f32::MAX;
    for (i, c) in centroids.iter().enumerate() {
        let d = (px[0] - c[0]).powi(2) + (px[1] - c[1]).powi(2) + (px[2] - c[2]).powi(2);
        if d < best_dist {
            best_dist = d;
            best = i;
        }
    }
    best
}

#[inline]
fn luminance(rgb: &[f32; 3]) -> f32 {
    0.299 * rgb[0] + 0.587 * rgb[1] + 0.114 * rgb[2]
}

fn otsu_threshold_gray(img: &image::GrayImage) -> u8 {
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
    let mut best = 0u8;

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
            best = t as u8;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kmeans_basic() {
        // Two clusters: dark and bright
        let pixels: Vec<[f32; 3]> = (0..100)
            .map(|i| {
                if i < 50 {
                    [10.0, 10.0, 10.0]
                } else {
                    [240.0, 240.0, 240.0]
                }
            })
            .collect();
        let centroids = kmeans(&pixels, 2, 15);
        assert_eq!(centroids.len(), 2);
        // One centroid should be near 10, the other near 240
        let mut lums: Vec<f32> = centroids.iter().map(|c| luminance(c)).collect();
        lums.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!(lums[0] < 50.0, "Dark centroid lum={}", lums[0]);
        assert!(lums[1] > 200.0, "Bright centroid lum={}", lums[1]);
    }

    #[test]
    fn test_nearest_centroid() {
        let centroids = vec![[0.0, 0.0, 0.0], [255.0, 255.0, 255.0]];
        assert_eq!(nearest_centroid(&[10.0, 10.0, 10.0], &centroids), 0);
        assert_eq!(nearest_centroid(&[200.0, 200.0, 200.0], &centroids), 1);
    }
}
