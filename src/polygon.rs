use crate::artwork::ArtworkBitmap;
use crate::pdk::PdkConfig;
use anyhow::Result;
use rayon::prelude::*;

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

    #[inline]
    pub fn width(&self) -> i32 {
        self.x1 - self.x0
    }

    #[inline]
    pub fn height(&self) -> i32 {
        self.y1 - self.y0
    }

    #[inline]
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
/// - Each "on" pixel maps to a rectangle of exactly `min_width x min_width`
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
        "Generating polygons: pixel={}um, pitch={}um ({} dbu), strategy={:?}, touching={}",
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

const PARALLEL_PIXEL_THRESHOLD: usize = 800_000;

/// Height of each horizontal strip for parallel greedy merge
const STRIP_HEIGHT: usize = 256;

/// Simplest: one rectangle per pixel
fn pixel_rects(bitmap: &ArtworkBitmap, pixel_w: i32, pitch: i32) -> Vec<Rect> {
    let total = bitmap.width as usize * bitmap.height as usize;
    if total < PARALLEL_PIXEL_THRESHOLD {
        return pixel_rects_serial(bitmap, pixel_w, pitch);
    }
    (0..bitmap.height)
        .into_par_iter()
        .flat_map_iter(|y| {
            let y0 = (bitmap.height - 1 - y) as i32 * pitch;
            (0..bitmap.width).filter_map(move |x| {
                if bitmap.get(x, y) {
                    let x0 = x as i32 * pitch;
                    Some(Rect::new(x0, y0, x0 + pixel_w, y0 + pixel_w))
                } else {
                    None
                }
            })
        })
        .collect()
}

fn pixel_rects_serial(bitmap: &ArtworkBitmap, pixel_w: i32, pitch: i32) -> Vec<Rect> {
    let mut rects = Vec::with_capacity(bitmap.metal_count());
    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            if bitmap.get(x, y) {
                let x0 = x as i32 * pitch;
                let y0 = (bitmap.height - 1 - y) as i32 * pitch;
                rects.push(Rect::new(x0, y0, x0 + pixel_w, y0 + pixel_w));
            }
        }
    }
    rects
}

/// Merge horizontally adjacent pixels into runs
fn row_merged_rects(bitmap: &ArtworkBitmap, pixel_w: i32, pitch: i32) -> Vec<Rect> {
    let mut rects = Vec::with_capacity(bitmap.metal_count());
    for y in 0..bitmap.height {
        let y0 = (bitmap.height - 1 - y) as i32 * pitch;
        let mut run_start: Option<u32> = None;
        for x in 0..=bitmap.width {
            let on = x < bitmap.width && bitmap.get(x, y);
            match (on, run_start) {
                (true, None) => run_start = Some(x),
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

/// Set bits [start, start+len) in a bitset using word-level operations.
#[inline]
fn bulk_set_bits(used: &mut [u64], start: usize, len: usize) {
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
        used[first_word] |= mask;
        return;
    }

    used[first_word] |= !0u64 << first_bit;
    for w in &mut used[first_word + 1..last_word] {
        *w = !0u64;
    }
    used[last_word] |= if last_bit_incl == 63 {
        !0u64
    } else {
        (1u64 << (last_bit_incl + 1)) - 1
    };
}

/// Word-level scan of `used` bitset to find the first set bit in [start, start+raw).
/// Returns the distance to that bit (or `raw` if none found).
#[inline]
fn effective_run_word_scan(used: &[u64], start: usize, raw: usize) -> u16 {
    if raw == 0 {
        return 0;
    }
    let end = start + raw;
    let first_word = start / 64;
    let first_bit = start % 64;
    let last_word = (end - 1) / 64;

    // First word: mask off bits before start
    let first_masked = used[first_word] >> first_bit;

    if first_word == last_word {
        let bits_needed = raw;
        let masked = if bits_needed < 64 {
            first_masked & ((1u64 << bits_needed) - 1)
        } else {
            first_masked
        };
        return if masked != 0 {
            masked.trailing_zeros() as u16
        } else {
            raw as u16
        };
    }

    if first_masked != 0 {
        return first_masked.trailing_zeros() as u16;
    }
    let mut bits_checked = 64 - first_bit;

    for &word in &used[first_word + 1..last_word] {
        if word != 0 {
            return (bits_checked + word.trailing_zeros() as usize) as u16;
        }
        bits_checked += 64;
    }

    // Last word
    let remaining = raw - bits_checked;
    if remaining > 0 {
        let word = used[last_word];
        let masked = if remaining < 64 {
            word & ((1u64 << remaining) - 1)
        } else {
            word
        };
        if masked != 0 {
            return (bits_checked + masked.trailing_zeros() as usize) as u16;
        }
    }

    raw as u16
}

/// Greedy maximal rectangle merging (row-then-column)
fn greedy_merged_rects(bitmap: &ArtworkBitmap, pixel_w: i32, pitch: i32) -> Vec<Rect> {
    let w = bitmap.width as usize;
    let h = bitmap.height as usize;
    let total = w * h;

    debug_assert!(bitmap.width <= u16::MAX as u32);

    // Compute horizontal runs using u16 (halves memory vs u32)
    let mut runs = vec![0u16; total];
    let words = bitmap.words();

    if total >= PARALLEL_PIXEL_THRESHOLD {
        // Parallel runs computation - each row is independent
        runs.par_chunks_mut(w).enumerate().for_each(|(y, row_runs)| {
            let row_start = y * w;
            let mut count = 0u16;
            for x in (0..w).rev() {
                let bit_idx = row_start + x;
                if words[bit_idx / 64] & (1u64 << (bit_idx % 64)) != 0 {
                    count += 1;
                } else {
                    count = 0;
                }
                row_runs[x] = count;
            }
        });
    } else {
        for y in 0..h {
            let row_start = y * w;
            let mut count = 0u16;
            for x in (0..w).rev() {
                let bit_idx = row_start + x;
                if words[bit_idx / 64] & (1u64 << (bit_idx % 64)) != 0 {
                    count += 1;
                } else {
                    count = 0;
                }
                runs[row_start + x] = count;
            }
        }
    }

    // For large bitmaps, use strip-based parallel greedy merge
    if total >= PARALLEL_PIXEL_THRESHOLD && h > STRIP_HEIGHT {
        greedy_merge_parallel_strips(words, &runs, w, h, pixel_w, pitch)
    } else {
        greedy_merge_strip(words, &runs, w, 0, h, h, pixel_w, pitch)
    }
}

/// Run greedy merge across horizontal strips in parallel
fn greedy_merge_parallel_strips(
    words: &[u64],
    runs: &[u16],
    w: usize,
    h: usize,
    pixel_w: i32,
    pitch: i32,
) -> Vec<Rect> {
    let num_strips = h.div_ceil(STRIP_HEIGHT);
    (0..num_strips)
        .into_par_iter()
        .flat_map_iter(|strip_idx| {
            let y_start = strip_idx * STRIP_HEIGHT;
            let y_end = (y_start + STRIP_HEIGHT).min(h);
            let strip_h = y_end - y_start;
            greedy_merge_strip(words, runs, w, y_start, strip_h, h, pixel_w, pitch)
        })
        .collect()
}

/// Greedy merge within a single horizontal strip.
/// `y_start` is the global y offset; `strip_h` is the strip height.
/// `used` bitset is strip-local (strip_h * w bits).
#[allow(clippy::too_many_arguments)]
fn greedy_merge_strip(
    words: &[u64],
    runs: &[u16],
    w: usize,
    y_start: usize,
    strip_h: usize,
    total_h: usize,
    pixel_w: i32,
    pitch: i32,
) -> Vec<Rect> {
    let strip_pixels = strip_h * w;
    let mut used = vec![0u64; strip_pixels.div_ceil(64)];
    let mut rects = Vec::new();

    for sy in 0..strip_h {
        let gy = y_start + sy;
        for x in 0..w {
            let strip_bit = sy * w + x;
            if used[strip_bit / 64] & (1u64 << (strip_bit % 64)) != 0 {
                continue;
            }
            let global_bit = gy * w + x;
            if words[global_bit / 64] & (1u64 << (global_bit % 64)) == 0 {
                continue;
            }

            // Find widest rectangle extending down within this strip
            let run_raw = runs[gy * w + x] as usize;
            let mut min_run =
                effective_run_word_scan(&used, sy * w + x, run_raw);
            let mut best_area = 0u64;
            let mut best_w = 0u16;
            let mut best_h = 0u32;

            for dy in 0..(strip_h - sy) {
                let cy_strip = sy + dy;
                let cy_global = gy + dy;
                let strip_idx = cy_strip * w + x;
                let global_idx = cy_global * w + x;

                if words[global_idx / 64] & (1u64 << (global_idx % 64)) == 0
                    || used[strip_idx / 64] & (1u64 << (strip_idx % 64)) != 0
                {
                    break;
                }

                let row_run = runs[global_idx] as usize;
                min_run =
                    min_run.min(effective_run_word_scan(&used, strip_idx, row_run));
                let area = min_run as u64 * (dy as u64 + 1);
                if area > best_area {
                    best_area = area;
                    best_w = min_run;
                    best_h = dy as u32 + 1;
                }
            }

            // Mark used with bulk word-level operations
            for dy in 0..best_h as usize {
                let row_off = (sy + dy) * w;
                bulk_set_bits(&mut used, row_off + x, best_w as usize);
            }

            // Create rectangle
            let x0 = x as i32 * pitch;
            let y_flipped = (total_h - 1 - gy) as i32;
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
        ArtworkBitmap::from_bools(4, 2, &[true, true, true, false, false, true, true, true])
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
        let bmp = ArtworkBitmap::from_bools(3, 3, &vec![true; 9]);
        let rects = greedy_merged_rects(&bmp, 100, 100);
        // Should produce a single rectangle
        assert_eq!(rects.len(), 1);
    }

    #[test]
    fn test_bulk_set_bits_single_word() {
        let mut bits = vec![0u64; 2];
        bulk_set_bits(&mut bits, 3, 5); // set bits 3..8
        assert_eq!(bits[0], 0b11111000);
        assert_eq!(bits[1], 0);
    }

    #[test]
    fn test_bulk_set_bits_cross_word() {
        let mut bits = vec![0u64; 2];
        bulk_set_bits(&mut bits, 60, 10); // set bits 60..70
        assert_eq!(bits[0] >> 60, 0xF); // bits 60-63
        assert_eq!(bits[1] & 0x3F, 0x3F); // bits 64-69
    }

    #[test]
    fn test_bulk_set_bits_full_words() {
        let mut bits = vec![0u64; 4];
        bulk_set_bits(&mut bits, 0, 256); // set all 256 bits
        assert!(bits.iter().all(|&w| w == !0u64));
    }

    #[test]
    fn test_effective_run_word_scan_no_used() {
        let used = vec![0u64; 2];
        assert_eq!(effective_run_word_scan(&used, 0, 50), 50);
        assert_eq!(effective_run_word_scan(&used, 10, 80), 80);
    }

    #[test]
    fn test_effective_run_word_scan_with_used() {
        let mut used = vec![0u64; 2];
        used[0] = 1u64 << 20; // bit 20 is set
        assert_eq!(effective_run_word_scan(&used, 10, 50), 10); // hit at offset 10
        assert_eq!(effective_run_word_scan(&used, 21, 30), 30); // past the set bit
    }

    #[test]
    fn test_effective_run_word_scan_cross_word() {
        let mut used = vec![0u64; 2];
        used[1] = 1u64 << 5; // bit 69 globally
        assert_eq!(effective_run_word_scan(&used, 60, 20), 9); // 69 - 60 = 9
    }

    #[test]
    fn test_greedy_merge_l_shape() {
        // L-shape pattern to verify correctness with word-level ops
        let bmp = ArtworkBitmap::from_bools(
            4,
            4,
            &[
                true, false, false, false, true, false, false, false, true, true, true, true,
                false, false, false, false,
            ],
        );
        let rects = greedy_merged_rects(&bmp, 100, 100);
        // Verify total pixel coverage
        let total_pixels: i64 = rects.iter().map(|r| r.area() / (100 * 100)).sum();
        assert_eq!(total_pixels, 6); // 6 "on" pixels
    }

    #[test]
    fn test_greedy_merge_large_pattern() {
        // 80% density pattern at 128x128 - tests parallel-runs and strip merge paths
        // for smaller-than-threshold sizes, ensuring serial path works
        let size = 128u32;
        let bools: Vec<bool> = (0..size * size)
            .map(|i| {
                let x = i % size;
                let y = i / size;
                (x + y) % 5 != 0
            })
            .collect();
        let bmp = ArtworkBitmap::from_bools(size, size, &bools);
        let rects = greedy_merged_rects(&bmp, 10, 10);
        // Verify total pixel coverage matches
        let expected_on = bools.iter().filter(|&&b| b).count();
        let total_pixels: i64 = rects.iter().map(|r| r.area() / (10 * 10)).sum();
        assert_eq!(total_pixels as usize, expected_on);
    }
}
