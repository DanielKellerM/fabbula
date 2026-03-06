use crate::artwork::ArtworkBitmap;
use crate::pdk::PdkConfig;
use anyhow::Result;

/// A rectangle in database units (nm typically)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Rect {
    pub fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Self {
            x0: x0.min(x1),
            y0: y0.min(y1),
            x1: x0.max(x1),
            y1: y0.max(y1),
        }
    }

    pub fn width(&self) -> i32 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> i32 {
        self.y1 - self.y0
    }

    pub fn area(&self) -> i64 {
        self.width() as i64 * self.height() as i64
    }

    /// Can this rect merge horizontally with another (same y extent, adjacent x)?
    pub fn can_merge_right(&self, other: &Rect) -> bool {
        self.y0 == other.y0 && self.y1 == other.y1 && self.x1 == other.x0
    }

    /// Merge with an adjacent rect
    pub fn merge_right(&self, other: &Rect) -> Rect {
        Rect::new(self.x0, self.y0, other.x1, self.y1)
    }

    /// Convert to GDSII boundary coordinates (closed polygon, 5 points)
    pub fn to_gds_xy(&self) -> Vec<i32> {
        vec![
            self.x0, self.y0, // bottom-left
            self.x1, self.y0, // bottom-right
            self.x1, self.y1, // top-right
            self.x0, self.y1, // top-left
            self.x0, self.y0, // close
        ]
    }
}

/// Strategy for converting bitmap pixels to polygons
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolygonStrategy {
    /// Each pixel becomes one rectangle. Simple but produces many polygons.
    PixelRects,
    /// Merge adjacent pixels into horizontal runs. Good balance of simplicity and efficiency.
    RowMerge,
    /// Merge into maximal rectangles. Fewest polygons, most complex.
    GreedyMerge,
}

/// Generate DRC-clean polygons from a bitmap using the given PDK rules.
///
/// The key insight for DRC-clean output:
/// - Each "on" pixel maps to a rectangle of exactly `min_width × min_width`
/// - Adjacent pixels are placed with a pitch of `min_width + min_spacing`
/// - This guarantees: all polygons >= min_width, all gaps >= min_spacing
/// - Grid snapping ensures manufacturing grid compliance
///
/// For denser output (touching polygons), set `touching = true`:
/// - Adjacent "on" pixels produce touching/merged rectangles
/// - Gaps only appear where the bitmap has "off" pixels
/// - min_spacing is only guaranteed between non-adjacent metal regions
pub fn generate_polygons(
    bitmap: &ArtworkBitmap,
    pdk: &PdkConfig,
    strategy: PolygonStrategy,
    touching: bool,
) -> Result<Vec<Rect>> {
    let min_w_um = pdk.snap_to_grid(pdk.drc.min_width);
    let min_s_um = pdk.snap_to_grid(pdk.drc.min_spacing);

    // Pixel pitch: how far apart pixel centers are
    let pitch_um = if touching {
        min_w_um // pixels touch: pitch = width
    } else {
        min_w_um + min_s_um // pixels separated: guaranteed spacing
    };

    let pixel_w_dbu = pdk.um_to_dbu(min_w_um);
    let pitch_dbu = pdk.um_to_dbu(pitch_um);

    tracing::info!(
        "Generating polygons: pixel={}µm, pitch={}µm ({} dbu), strategy={:?}, touching={}",
        min_w_um,
        pitch_um,
        pitch_dbu,
        strategy,
        touching
    );

    let raw_rects = match strategy {
        PolygonStrategy::PixelRects => pixel_rects(bitmap, pixel_w_dbu, pitch_dbu),
        PolygonStrategy::RowMerge => row_merged_rects(bitmap, pixel_w_dbu, pitch_dbu),
        PolygonStrategy::GreedyMerge => greedy_merged_rects(bitmap, pixel_w_dbu, pitch_dbu),
    };

    // Filter out polygons that violate min_area
    let min_area_dbu2 = pdk.um_to_dbu(pdk.drc.min_area.sqrt()).pow(2) as i64;
    let filtered: Vec<Rect> = if pdk.drc.min_area > 0.0 {
        raw_rects
            .into_iter()
            .filter(|r| r.area() >= min_area_dbu2)
            .collect()
    } else {
        raw_rects
    };

    tracing::info!("Generated {} polygons", filtered.len());
    Ok(filtered)
}

/// Simplest: one rectangle per pixel
fn pixel_rects(bitmap: &ArtworkBitmap, pixel_w: i32, pitch: i32) -> Vec<Rect> {
    let mut rects = Vec::new();
    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            if bitmap.get(x, y) {
                let x0 = x as i32 * pitch;
                let y0 = (bitmap.height - 1 - y) as i32 * pitch; // flip Y
                rects.push(Rect::new(x0, y0, x0 + pixel_w, y0 + pixel_w));
            }
        }
    }
    rects
}

/// Merge horizontally adjacent pixels into runs
fn row_merged_rects(bitmap: &ArtworkBitmap, pixel_w: i32, pitch: i32) -> Vec<Rect> {
    let mut rects = Vec::new();

    for y in 0..bitmap.height {
        let y0 = (bitmap.height - 1 - y) as i32 * pitch;
        let mut run_start: Option<u32> = None;

        for x in 0..=bitmap.width {
            let on = x < bitmap.width && bitmap.get(x, y);

            match (on, run_start) {
                (true, None) => {
                    run_start = Some(x);
                }
                (false, Some(start)) => {
                    let x0 = start as i32 * pitch;
                    let x1 = (x - 1) as i32 * pitch + pixel_w;
                    rects.push(Rect::new(x0, y0, x1, y0 + pixel_w));
                    run_start = None;
                }
                _ => {}
            }
        }
    }

    rects
}

/// Greedy maximal rectangle merging (row-then-column)
fn greedy_merged_rects(bitmap: &ArtworkBitmap, pixel_w: i32, pitch: i32) -> Vec<Rect> {
    let w = bitmap.width as usize;
    let h = bitmap.height as usize;

    // First pass: compute horizontal runs
    // runs[y][x] = number of consecutive "on" pixels starting at (x, y) going right
    let mut runs = vec![vec![0u32; w]; h];
    for (y, row) in runs.iter_mut().enumerate() {
        let mut count = 0u32;
        for x in (0..w).rev() {
            if bitmap.get(x as u32, y as u32) {
                count += 1;
            } else {
                count = 0;
            }
            row[x] = count;
        }
    }

    // Second pass: for each cell, try to extend downward to form maximal rects
    let mut used = vec![vec![false; w]; h];
    let mut rects = Vec::new();

    for y in 0..h {
        for x in 0..w {
            if used[y][x] || !bitmap.get(x as u32, y as u32) {
                continue;
            }

            // Find the widest rectangle starting at (x, y) extending down
            let mut min_run = runs[y][x];
            let mut best_area = 0u64;
            let mut best_w = 0u32;
            let mut best_h = 0u32;

            for dy in 0..(h - y) {
                let cy = y + dy;
                if !bitmap.get(x as u32, cy as u32) || used[cy][x] {
                    break;
                }
                min_run = min_run.min(runs[cy][x]);
                let area = min_run as u64 * (dy as u64 + 1);
                if area > best_area {
                    best_area = area;
                    best_w = min_run;
                    best_h = dy as u32 + 1;
                }
            }

            // Mark used
            for dy in 0..best_h as usize {
                for dx in 0..best_w as usize {
                    used[y + dy][x + dx] = true;
                }
            }

            // Create rectangle
            let x0 = x as i32 * pitch;
            let y_flipped = (h - 1 - y) as i32;
            let y0 = (y_flipped - best_h as i32 + 1) * pitch;
            let x1 = (x as i32 + best_w as i32 - 1) * pitch + pixel_w;
            let y1 = y_flipped * pitch + pixel_w;

            rects.push(Rect::new(x0, y0, x1, y1));
        }
    }

    rects
}

/// Compute bounding box of all polygons
pub fn bounding_box(rects: &[Rect]) -> Option<Rect> {
    let first = *rects.first()?;
    Some(rects[1..].iter().fold(first, |bb, r| Rect {
        x0: bb.x0.min(r.x0),
        y0: bb.y0.min(r.y0),
        x1: bb.x1.max(r.x1),
        y1: bb.y1.max(r.y1),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_bitmap() -> ArtworkBitmap {
        ArtworkBitmap {
            width: 4,
            height: 2,
            pixels: vec![true, true, true, false, false, true, true, true],
        }
    }

    #[test]
    fn test_pixel_rects() {
        let bmp = test_bitmap();
        let rects = pixel_rects(&bmp, 100, 200);
        assert_eq!(rects.len(), 6); // 6 "on" pixels
    }

    #[test]
    fn test_row_merge() {
        let bmp = test_bitmap();
        let rects = row_merged_rects(&bmp, 100, 200);
        // Row 0: [0..2] = 1 rect, Row 1: [1..3] = 1 rect
        assert_eq!(rects.len(), 2);
    }

    #[test]
    fn test_greedy_merge() {
        let bmp = ArtworkBitmap {
            width: 3,
            height: 3,
            pixels: vec![true, true, true, true, true, true, true, true, true],
        };
        let rects = greedy_merged_rects(&bmp, 100, 100);
        // Should produce a single rectangle
        assert_eq!(rects.len(), 1);
    }
}
