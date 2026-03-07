// Copyright 2026 Daniel Keller <daniel.keller.m@gmail.com>
// Licensed under the Apache License, Version 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Tile pyramid generation for deep zoom previews.
//!
//! Renders artwork polygons into a PNG tile pyramid (like map tiles) for
//! efficient browser viewing. Uses `tiny_skia` (via `resvg`) for rasterization.

use crate::polygon::Rect;
use anyhow::{Context, Result};
use std::io::Write;
use std::path::Path;

/// Configuration for tile generation.
pub struct TileConfig {
    /// Tile size in pixels (default 256).
    pub tile_size: u32,
    /// Maximum full-image resolution in pixels (longest side).
    pub max_resolution: u32,
}

impl Default for TileConfig {
    fn default() -> Self {
        Self {
            tile_size: 256,
            max_resolution: 4096,
        }
    }
}

/// Metadata for a generated tile pyramid.
#[derive(Debug)]
pub struct TilePyramid {
    pub width: u32,
    pub height: u32,
    pub tile_size: u32,
    pub num_levels: u32,
    pub density_grid: Vec<Vec<u32>>,
}

/// A layer of rectangles with an associated color for rendering.
pub struct TileLayer<'a> {
    pub rects: &'a [Rect],
    pub color: [u8; 3],
    pub name: &'a str,
}

/// Parse a hex color string (e.g. "#c0c0c0") into [R, G, B].
pub fn parse_hex_color(s: &str) -> [u8; 3] {
    let s = s.trim_start_matches('#');
    if s.len() >= 6 {
        let r = u8::from_str_radix(&s[0..2], 16).unwrap_or(192);
        let g = u8::from_str_radix(&s[2..4], 16).unwrap_or(192);
        let b = u8::from_str_radix(&s[4..6], 16).unwrap_or(192);
        [r, g, b]
    } else {
        [192, 192, 192]
    }
}

/// Render all layers into a full-resolution pixmap.
fn render_full_image(
    layers: &[TileLayer],
    bb: &Rect,
    resolution: u32,
    dark_bg: bool,
) -> Result<resvg::tiny_skia::Pixmap> {
    use resvg::tiny_skia::{Color, Paint, PathBuilder, Pixmap, Transform};

    let art_w = bb.width() as f64;
    let art_h = bb.height() as f64;
    let aspect = art_w / art_h;

    let (px_w, px_h) = if aspect >= 1.0 {
        (resolution, (resolution as f64 / aspect).ceil() as u32)
    } else {
        ((resolution as f64 * aspect).ceil() as u32, resolution)
    };

    let mut pixmap =
        Pixmap::new(px_w.max(1), px_h.max(1)).context("Failed to create pixmap for tiles")?;

    // Fill background
    let bg = if dark_bg {
        Color::from_rgba8(13, 17, 23, 255)
    } else {
        Color::from_rgba8(245, 245, 240, 255)
    };
    pixmap.fill(bg);

    let scale_x = px_w as f64 / art_w;
    let scale_y = px_h as f64 / art_h;

    for layer in layers {
        let [r, g, b] = layer.color;
        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba8(r, g, b, 217)); // ~0.85 opacity
        paint.anti_alias = false;

        for rect in layer.rects {
            // Transform from artwork coords to pixel coords (y-flip)
            let px_x = (rect.x0 - bb.x0) as f64 * scale_x;
            let px_y = (bb.y1 - rect.y1) as f64 * scale_y; // flip Y
            let px_w = rect.width() as f64 * scale_x;
            let px_h = rect.height() as f64 * scale_y;

            let Some(r) = resvg::tiny_skia::Rect::from_xywh(
                px_x as f32,
                px_y as f32,
                px_w.max(0.5) as f32,
                px_h.max(0.5) as f32,
            ) else {
                continue;
            };
            let path = PathBuilder::from_rect(r);
            pixmap.fill_path(
                &path,
                &paint,
                resvg::tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    Ok(pixmap)
}

/// Generate a tile pyramid from a full-resolution pixmap.
///
/// Creates directories like `0/`, `1/`, `2/` etc. with tiles named `col_row.png`.
/// Level 0 is the most zoomed-out (fewest tiles), max level is full resolution.
pub fn generate_tile_pyramid(
    layers: &[TileLayer],
    bb: &Rect,
    config: &TileConfig,
    output_dir: &Path,
) -> Result<TilePyramid> {
    let pixmap = render_full_image(layers, bb, config.max_resolution, true)?;
    let full_w = pixmap.width();
    let full_h = pixmap.height();

    // Determine number of levels: keep halving until we fit in one tile
    let max_dim = full_w.max(full_h);
    let num_levels = if max_dim <= config.tile_size {
        1
    } else {
        ((max_dim as f64) / (config.tile_size as f64)).log2().ceil() as u32 + 1
    };
    tracing::info!(
        "Tile pyramid: {}x{} px, {} levels, tile size {}",
        full_w,
        full_h,
        num_levels,
        config.tile_size
    );

    // Generate tiles from the highest resolution level down to level 0
    // We'll work with image crate for downsampling
    let mut current = image::RgbaImage::from_raw(full_w, full_h, pixmap.data().to_vec())
        .context("Failed to convert pixmap to image")?;

    // Iterate from max_level (full res) down to 0 (overview)
    for level in (0..num_levels).rev() {
        let level_dir = output_dir.join(level.to_string());
        std::fs::create_dir_all(&level_dir)
            .with_context(|| format!("Failed to create tile dir: {}", level_dir.display()))?;

        let img_w = current.width();
        let img_h = current.height();
        let ts = config.tile_size;
        let cols = img_w.div_ceil(ts);
        let rows = img_h.div_ceil(ts);

        for row in 0..rows {
            for col in 0..cols {
                let x = col * ts;
                let y = row * ts;
                let tw = ts.min(img_w - x);
                let th = ts.min(img_h - y);

                let tile = image::imageops::crop_imm(&current, x, y, tw, th).to_image();
                let tile_path = level_dir.join(format!("{}_{}.png", col, row));
                tile.save(&tile_path)
                    .with_context(|| format!("Failed to save tile: {}", tile_path.display()))?;
            }
        }

        tracing::debug!(
            "Level {}: {}x{} -> {}x{} tiles",
            level,
            img_w,
            img_h,
            cols,
            rows
        );

        // Downsample by 2x for the next (lower) level, unless we're at level 0
        if level > 0 {
            let new_w = (img_w / 2).max(1);
            let new_h = (img_h / 2).max(1);
            current = image::imageops::resize(
                &current,
                new_w,
                new_h,
                image::imageops::FilterType::Lanczos3,
            );
        }
    }

    // Compute density grid (32x32)
    let density_grid = compute_density_grid(layers, bb, 32);

    // Write metadata
    let meta = TilePyramid {
        width: full_w,
        height: full_h,
        tile_size: config.tile_size,
        num_levels,
        density_grid,
    };
    write_meta_json(&meta, output_dir)?;
    write_polygon_json(layers, bb, output_dir)?;

    // Also render a dark + light overview tile at level 0 for gallery thumbnails
    // The dark one is already there from the pyramid generation

    tracing::info!(
        "Wrote tile pyramid: {} ({} levels, {} tiles)",
        output_dir.display(),
        num_levels,
        count_tiles(full_w, full_h, config.tile_size, num_levels)
    );

    Ok(meta)
}

fn count_tiles(w: u32, h: u32, ts: u32, levels: u32) -> u32 {
    let mut total = 0;
    let mut cw = w;
    let mut ch = h;
    for _ in 0..levels {
        total += cw.div_ceil(ts) * ch.div_ceil(ts);
        cw = (cw / 2).max(1);
        ch = (ch / 2).max(1);
    }
    total
}

/// Compute a density grid counting polygons per cell.
fn compute_density_grid(layers: &[TileLayer], bb: &Rect, grid_size: u32) -> Vec<Vec<u32>> {
    let cell_w = bb.width() as f64 / grid_size as f64;
    let cell_h = bb.height() as f64 / grid_size as f64;
    let mut grid = vec![vec![0u32; grid_size as usize]; grid_size as usize];

    for layer in layers {
        for rect in layer.rects {
            let cx = ((rect.x0 - bb.x0) as f64 / cell_w) as u32;
            let cy = ((rect.y0 - bb.y0) as f64 / cell_h) as u32;
            let cx = cx.min(grid_size - 1) as usize;
            let cy = cy.min(grid_size - 1) as usize;
            grid[cy][cx] += 1;
        }
    }
    grid
}

/// Write meta.json with pyramid metadata.
fn write_meta_json(meta: &TilePyramid, output_dir: &Path) -> Result<()> {
    let path = output_dir.join("meta.json");
    let mut f = std::io::BufWriter::new(
        std::fs::File::create(&path)
            .with_context(|| format!("Failed to create {}", path.display()))?,
    );

    write!(
        f,
        r#"{{"width":{},"height":{},"tileSize":{},"maxLevel":{}"#,
        meta.width,
        meta.height,
        meta.tile_size,
        meta.num_levels - 1
    )?;

    // Write density grid inline
    write!(f, r#","densityGrid":["#)?;
    for (i, row) in meta.density_grid.iter().enumerate() {
        if i > 0 {
            write!(f, ",")?;
        }
        write!(f, "[")?;
        for (j, val) in row.iter().enumerate() {
            if j > 0 {
                write!(f, ",")?;
            }
            write!(f, "{}", val)?;
        }
        write!(f, "]")?;
    }
    writeln!(f, "]}}")?;

    Ok(())
}

/// Write polygon JSON for SVG overlay.
fn write_polygon_json(layers: &[TileLayer], bb: &Rect, output_dir: &Path) -> Result<()> {
    let path = output_dir.join("polygons.json");
    let mut f = std::io::BufWriter::new(
        std::fs::File::create(&path)
            .with_context(|| format!("Failed to create {}", path.display()))?,
    );

    // Write as compact JSON array: [{x0,y0,x1,y1,l}, ...]
    // Coordinates relative to bounding box origin for compactness
    write!(
        f,
        r#"{{"ox":{},"oy":{},"bb_w":{},"bb_h":{},"layers":["#,
        bb.x0,
        bb.y0,
        bb.width(),
        bb.height()
    )?;
    for (li, layer) in layers.iter().enumerate() {
        if li > 0 {
            write!(f, ",")?;
        }
        write!(
            f,
            "{{\"name\":\"{}\",\"color\":\"#{:02x}{:02x}{:02x}\",\"rects\":[",
            layer.name, layer.color[0], layer.color[1], layer.color[2]
        )?;
        for (ri, rect) in layer.rects.iter().enumerate() {
            if ri > 0 {
                write!(f, ",")?;
            }
            write!(
                f,
                "[{},{},{},{}]",
                rect.x0 - bb.x0,
                rect.y0 - bb.y0,
                rect.x1 - bb.x0,
                rect.y1 - bb.y0,
            )?;
        }
        write!(f, "]}}")?;
    }
    writeln!(f, "]}}")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_color() {
        assert_eq!(parse_hex_color("#c0c0c0"), [192, 192, 192]);
        assert_eq!(parse_hex_color("#ff0000"), [255, 0, 0]);
        assert_eq!(parse_hex_color("00ff00"), [0, 255, 0]);
    }

    #[test]
    fn test_density_grid() {
        let rects = vec![Rect::new(0, 0, 100, 100), Rect::new(50, 50, 150, 150)];
        let bb = Rect::new(0, 0, 200, 200);
        let layer = TileLayer {
            rects: &rects,
            color: [192, 192, 192],
            name: "test",
        };
        let grid = compute_density_grid(&[layer], &bb, 4);
        assert_eq!(grid.len(), 4);
        assert_eq!(grid[0].len(), 4);
        // Both rects start in the bottom-left quadrant region
        assert!(grid[0][0] > 0);
    }

    #[test]
    fn test_tile_pyramid_small() {
        let rects = vec![Rect::new(0, 0, 1000, 1000)];
        let bb = Rect::new(0, 0, 1000, 1000);
        let layer = TileLayer {
            rects: &rects,
            color: [192, 192, 192],
            name: "metal",
        };
        let dir = tempfile::tempdir().unwrap();
        let config = TileConfig {
            tile_size: 256,
            max_resolution: 512,
        };
        let result = generate_tile_pyramid(&[layer], &bb, &config, dir.path());
        assert!(result.is_ok());
        let meta = result.unwrap();
        assert!(meta.num_levels >= 1);
        // Check level 0 overview tile exists
        assert!(dir.path().join("0/0_0.png").exists());
        // Check meta.json exists
        assert!(dir.path().join("meta.json").exists());
        // Check polygons.json exists
        assert!(dir.path().join("polygons.json").exists());
    }
}
