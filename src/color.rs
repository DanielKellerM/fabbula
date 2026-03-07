// Copyright 2026 Daniel Keller <daniel.keller.m@gmail.com>
// Licensed under the Apache License, Version 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Multi-layer color extraction (channel splitting, palette quantization).
//!
//! Converts color images into per-layer bitmaps using either RGB channel
//! extraction ([`extract_channels`]) or k-means palette quantization
//! ([`extract_palette`]).

use crate::artwork::{ArtworkBitmap, ThresholdMode};
use crate::pdk::ArtworkLayerProfile;
use anyhow::Result;
use rayon::prelude::*;
use std::path::Path;

const KMEANS_PARALLEL_THRESHOLD: usize = 100_000;
const KMEANS_MAX_SAMPLES: usize = 50_000;

/// Color extraction mode for multi-layer artwork
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
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

/// Load image (raster or SVG) and open it with optional resize, returning the DynamicImage.
fn load_image(path: &Path, max_pixels: Option<(u32, u32)>) -> Result<image::DynamicImage> {
    let img = crate::artwork::load_image_file(
        path,
        if crate::artwork::is_svg(path) {
            max_pixels
        } else {
            None
        },
    )?;
    let img = if !crate::artwork::is_svg(path) {
        if let Some((max_w, max_h)) = max_pixels {
            let (w, h) = image::GenericImageView::dimensions(&img);
            if w > max_w || h > max_h {
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
        ThresholdMode::Otsu | ThresholdMode::Auto => {
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
    max_pixels: Option<(u32, u32)>,
) -> Result<Vec<LayerBitmap>> {
    anyhow::ensure!(num_layers >= 1, "num_layers must be >= 1");

    let img = load_image(path, max_pixels)?;
    let (width, height) = image::GenericImageView::dimensions(&img);
    let rgb = img.to_rgb8();
    let total_pixels = (width as usize) * (height as usize);

    // Quantize into num_layers + 1 clusters: the extra one captures background
    let k = num_layers + 1;

    // Subsample pixels for k-means training if the image is large
    let centroids = if total_pixels > KMEANS_MAX_SAMPLES {
        let step = total_pixels / KMEANS_MAX_SAMPLES;
        let samples: Vec<[f32; 3]> = rgb
            .pixels()
            .step_by(step.max(1))
            .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
            .collect();
        kmeans(&samples, k, 15)
    } else {
        let pixels: Vec<[f32; 3]> = rgb
            .pixels()
            .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
            .collect();
        kmeans(&pixels, k, 15)
    };

    // Assign each pixel to nearest centroid (directly from image iterator)
    let assignments: Vec<usize> = if total_pixels >= KMEANS_PARALLEL_THRESHOLD {
        let pixels: Vec<[f32; 3]> = rgb
            .pixels()
            .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
            .collect();
        pixels
            .par_iter()
            .map(|px| nearest_centroid(px, &centroids))
            .collect()
    } else {
        rgb.pixels()
            .map(|p| {
                let px = [p[0] as f32, p[1] as f32, p[2] as f32];
                nearest_centroid(&px, &centroids)
            })
            .collect()
    };

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
    let bg_cluster = *centroid_order
        .last()
        .expect("centroid_order is non-empty (k >= 2)");
    let bg_color = &centroids[bg_cluster];
    tracing::info!(
        "Palette background: centroid RGB({:.0},{:.0},{:.0}) - skipped",
        bg_color[0],
        bg_color[1],
        bg_color[2],
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
        .min_by(|a, b| {
            luminance(a)
                .partial_cmp(&luminance(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .expect("pixels is non-empty (checked above)");
    centroids.push(*first);

    for _ in 1..k {
        // Pick pixel with max distance to nearest existing centroid
        let best = pixels
            .iter()
            .max_by(|a, b| {
                let dist_a: f32 = centroids
                    .iter()
                    .map(|c| (a[0] - c[0]).powi(2) + (a[1] - c[1]).powi(2) + (a[2] - c[2]).powi(2))
                    .fold(f32::MAX, f32::min);
                let dist_b: f32 = centroids
                    .iter()
                    .map(|c| (b[0] - c[0]).powi(2) + (b[1] - c[1]).powi(2) + (b[2] - c[2]).powi(2))
                    .fold(f32::MAX, f32::min);
                dist_a
                    .partial_cmp(&dist_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .expect("pixels is non-empty (checked above)");
        centroids.push(*best);
    }

    let mut assignments = vec![0usize; n];
    let mut converged = false;

    for iter in 0..max_iters {
        // Assign pixels to nearest centroid
        let changed = if n >= KMEANS_PARALLEL_THRESHOLD {
            let new_assignments: Vec<usize> = pixels
                .par_iter()
                .map(|px| nearest_centroid(px, &centroids))
                .collect();
            let any_changed = new_assignments
                .iter()
                .zip(assignments.iter())
                .any(|(a, b)| a != b);
            assignments = new_assignments;
            any_changed
        } else {
            let mut changed = false;
            for (i, px) in pixels.iter().enumerate() {
                let nearest = nearest_centroid(px, &centroids);
                if nearest != assignments[i] {
                    assignments[i] = nearest;
                    changed = true;
                }
            }
            changed
        };

        if !changed {
            tracing::debug!("K-means converged after {} iterations", iter + 1);
            converged = true;
            break;
        }

        // Update centroids - parallel accumulation for large datasets
        let (sums, counts) = if n >= KMEANS_PARALLEL_THRESHOLD {
            pixels
                .par_chunks(8192)
                .zip(assignments.par_chunks(8192))
                .map(|(px_chunk, assign_chunk)| {
                    let mut local_sums = vec![[0.0f64; 3]; k];
                    let mut local_counts = vec![0u64; k];
                    for (px, &c) in px_chunk.iter().zip(assign_chunk.iter()) {
                        local_sums[c][0] += px[0] as f64;
                        local_sums[c][1] += px[1] as f64;
                        local_sums[c][2] += px[2] as f64;
                        local_counts[c] += 1;
                    }
                    (local_sums, local_counts)
                })
                .reduce(
                    || (vec![[0.0f64; 3]; k], vec![0u64; k]),
                    |(mut sums_a, mut counts_a), (sums_b, counts_b)| {
                        for j in 0..k {
                            sums_a[j][0] += sums_b[j][0];
                            sums_a[j][1] += sums_b[j][1];
                            sums_a[j][2] += sums_b[j][2];
                            counts_a[j] += counts_b[j];
                        }
                        (sums_a, counts_a)
                    },
                )
        } else {
            let mut sums = vec![[0.0f64; 3]; k];
            let mut counts = vec![0u64; k];
            for (i, px) in pixels.iter().enumerate() {
                let c = assignments[i];
                sums[c][0] += px[0] as f64;
                sums[c][1] += px[1] as f64;
                sums[c][2] += px[2] as f64;
                counts[c] += 1;
            }
            (sums, counts)
        };

        // Pre-compute replacements for empty clusters (diverse pixels)
        let empty_indices: Vec<usize> = counts
            .iter()
            .enumerate()
            .filter(|&(_, c)| *c == 0)
            .map(|(i, _)| i)
            .collect();
        let mut replacements: Vec<[f32; 3]> = Vec::new();
        if !empty_indices.is_empty() {
            // Sort pixels by distance from centroids, pick farthest ones
            let mut scored: Vec<(usize, f32)> = pixels
                .iter()
                .enumerate()
                .map(|(i, px)| (i, min_centroid_dist(px, &centroids)))
                .collect();
            scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            // Pick up to len(empty_indices) distinct replacement pixels
            for i in 0..empty_indices.len() {
                let idx = scored.get(i).map(|s| s.0).unwrap_or(0);
                replacements.push(pixels[idx]);
            }
        }
        let mut repl_iter = 0;
        for (j, centroid) in centroids.iter_mut().enumerate() {
            if counts[j] > 0 {
                centroid[0] = (sums[j][0] / counts[j] as f64) as f32;
                centroid[1] = (sums[j][1] / counts[j] as f64) as f32;
                centroid[2] = (sums[j][2] / counts[j] as f64) as f32;
            } else if repl_iter < replacements.len() {
                *centroid = replacements[repl_iter];
                repl_iter += 1;
            }
        }
    }

    if !converged {
        tracing::warn!(
            "K-means did not converge after {} iterations; layer separation may be poor",
            max_iters
        );
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
fn min_centroid_dist(px: &[f32; 3], centroids: &[[f32; 3]]) -> f32 {
    centroids
        .iter()
        .map(|c| (px[0] - c[0]).powi(2) + (px[1] - c[1]).powi(2) + (px[2] - c[2]).powi(2))
        .fold(f32::MAX, f32::min)
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
        let mut lums: Vec<f32> = centroids.iter().map(luminance).collect();
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

    use crate::pdk::{ArtworkLayerProfile, DrcRules};
    use image::RgbImage;

    fn test_drc_rules() -> DrcRules {
        DrcRules {
            min_width: 1.0,
            min_spacing: 0.5,
            min_area: 0.0,
            density_min: 0.0,
            density_max: 1.0,
            density_window_um: 500.0,
            max_width: None,
            wide_metal_threshold: None,
            wide_metal_spacing: None,
        }
    }

    fn test_profile(name: &str, color: Option<&str>) -> ArtworkLayerProfile {
        ArtworkLayerProfile {
            name: name.into(),
            gds_layer: 72,
            gds_datatype: 20,
            purpose: String::new(),
            color: color.map(|c| c.into()),
            drc: test_drc_rules(),
        }
    }

    #[test]
    fn test_extract_palette_two_colors() {
        // Create a 10x10 image: left half black, right half white
        let img = image::ImageBuffer::from_fn(10, 10, |x, _y| {
            if x < 5 {
                image::Rgb([0u8, 0, 0])
            } else {
                image::Rgb([255u8, 255, 255])
            }
        });
        let tmp = tempfile::NamedTempFile::with_suffix(".png").unwrap();
        img.save(tmp.path()).unwrap();

        let results = extract_palette(tmp.path(), 1, None).unwrap();
        assert_eq!(
            results.len(),
            1,
            "Expected 1 layer bitmap from 2-color image with num_layers=1"
        );
        let density = results[0].bitmap.density();
        assert!(
            density > 0.0 && density < 1.0,
            "Density should be between 0 and 1, got {}",
            density
        );
    }

    #[test]
    fn test_extract_palette_three_colors() {
        // Create a 9x3 image with 3 distinct color regions: red, green, blue
        let img = image::ImageBuffer::from_fn(9, 3, |x, _y| {
            if x < 3 {
                image::Rgb([255u8, 0, 0])
            } else if x < 6 {
                image::Rgb([0u8, 255, 0])
            } else {
                image::Rgb([0u8, 0, 255])
            }
        });
        let tmp = tempfile::NamedTempFile::with_suffix(".png").unwrap();
        img.save(tmp.path()).unwrap();

        // num_layers=2 means 3 clusters total, brightest dropped as background
        let results = extract_palette(tmp.path(), 2, None).unwrap();
        assert_eq!(
            results.len(),
            2,
            "Expected 2 layer bitmaps from 3-color image with num_layers=2"
        );
        // Each layer should have non-zero density
        for (i, lb) in results.iter().enumerate() {
            assert!(
                lb.bitmap.density() > 0.0,
                "Layer {} density should be > 0, got {}",
                i,
                lb.bitmap.density()
            );
        }
    }

    #[test]
    fn test_extract_channels_rgb() {
        let profiles = vec![
            test_profile("met5_r", Some("red")),
            test_profile("met5_g", Some("green")),
            test_profile("met5_b", Some("blue")),
        ];

        // Create a 4x4 image with varied colors
        let img = image::ImageBuffer::from_fn(4, 4, |x, y| {
            let r = if x < 2 { 255u8 } else { 0 };
            let g = if y < 2 { 255u8 } else { 0 };
            let b = if (x + y) % 2 == 0 { 255u8 } else { 0 };
            image::Rgb([r, g, b])
        });
        let tmp = tempfile::NamedTempFile::with_suffix(".png").unwrap();
        img.save(tmp.path()).unwrap();

        let results =
            extract_channels(tmp.path(), &profiles, ThresholdMode::Luminance(128), None).unwrap();
        assert_eq!(
            results.len(),
            3,
            "Expected 3 LayerBitmaps for R/G/B profiles"
        );
        // Verify each result maps to the correct profile index
        let indices: Vec<usize> = results.iter().map(|lb| lb.layer_index).collect();
        assert!(indices.contains(&0), "Should contain layer_index 0 (red)");
        assert!(indices.contains(&1), "Should contain layer_index 1 (green)");
        assert!(indices.contains(&2), "Should contain layer_index 2 (blue)");
    }

    #[test]
    fn test_extract_channels_no_color_field() {
        let profiles = vec![test_profile("met5_a", None), test_profile("met5_b", None)];

        // Create a minimal image
        let img: RgbImage = RgbImage::new(2, 2);
        let tmp = tempfile::NamedTempFile::with_suffix(".png").unwrap();
        img.save(tmp.path()).unwrap();

        let result = extract_channels(tmp.path(), &profiles, ThresholdMode::Luminance(128), None);
        assert!(
            result.is_err(),
            "extract_channels should fail when no profiles have a color field"
        );
        let err_msg = format!("{}", result.err().unwrap());
        assert!(
            err_msg.contains("color"),
            "Error message should mention 'color', got: {}",
            err_msg
        );
    }
}
