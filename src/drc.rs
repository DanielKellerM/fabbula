// Copyright 2026 Daniel Keller <daniel.keller.m@gmail.com>
// Licensed under the Apache License, Version 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Design rule checking with R-tree spatial indexing.
//!
//! Validates generated polygons against PDK design rules: minimum/maximum width,
//! minimum spacing, wide-metal spacing, minimum area, and metal density.

use crate::pdk::{DrcRules, um_to_dbu};
use crate::polygon::{Rect, bounding_box};
use rayon::prelude::*;
use rstar::{AABB, RTree, RTreeObject};

const PARALLEL_RECT_THRESHOLD: usize = 5_000;

/// DRC rule categories
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrcRule {
    MinWidth,
    MaxWidth,
    MinSpacing,
    WideMetalSpacing,
    MinArea,
    DensityMax,
    DensityMin,
}

impl std::fmt::Display for DrcRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MinWidth => write!(f, "min_width"),
            Self::MaxWidth => write!(f, "max_width"),
            Self::MinSpacing => write!(f, "min_spacing"),
            Self::WideMetalSpacing => write!(f, "wide_metal_spacing"),
            Self::MinArea => write!(f, "min_area"),
            Self::DensityMax => write!(f, "density_max"),
            Self::DensityMin => write!(f, "density_min"),
        }
    }
}

/// A DRC violation - stores raw data, formats only on Display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrcViolation {
    pub rule: DrcRule,
    pub rect_index: u32,
    pub other_index: u32,
    pub value: i64,
    pub limit: i64,
    pub location: (i32, i32),
}

impl std::fmt::Display for DrcViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.rule {
            DrcRule::MinWidth => write!(
                f,
                "Rect {} dimension {} dbu < min {} dbu",
                self.rect_index, self.value, self.limit
            ),
            DrcRule::MaxWidth => write!(
                f,
                "Rect {} dimension {} dbu exceeds max {} dbu",
                self.rect_index, self.value, self.limit
            ),
            DrcRule::MinSpacing => write!(
                f,
                "Rects {}-{} spacing {} dbu < min {} dbu",
                self.rect_index, self.other_index, self.value, self.limit
            ),
            DrcRule::WideMetalSpacing => write!(
                f,
                "Rects {}-{} spacing {} dbu < wide-metal min {} dbu",
                self.rect_index, self.other_index, self.value, self.limit
            ),
            DrcRule::MinArea => write!(
                f,
                "Rect {} area {} dbu^2 < min {} dbu^2",
                self.rect_index, self.value, self.limit
            ),
            DrcRule::DensityMax => write!(
                f,
                "Density {:.1}% exceeds max {:.1}% at ({}, {})",
                self.value as f64 / 10.0,
                self.limit as f64 / 10.0,
                self.location.0,
                self.location.1
            ),
            DrcRule::DensityMin => write!(
                f,
                "Density {:.1}% below min {:.1}% at ({}, {})",
                self.value as f64 / 10.0,
                self.limit as f64 / 10.0,
                self.location.0,
                self.location.1
            ),
        }
    }
}

/// Wrapper for Rect that implements RTreeObject
#[derive(Debug, Clone, Copy)]
struct IndexedRect {
    index: u32,
    rect: Rect,
}

impl RTreeObject for IndexedRect {
    type Envelope = AABB<[i32; 2]>;

    fn envelope(&self) -> Self::Envelope {
        AABB::from_corners([self.rect.x0, self.rect.y0], [self.rect.x1, self.rect.y1])
    }
}

fn build_rtree(rects: &[Rect]) -> RTree<IndexedRect> {
    let indexed: Vec<IndexedRect> = rects
        .iter()
        .enumerate()
        .map(|(i, r)| IndexedRect {
            index: i as u32,
            rect: *r,
        })
        .collect();
    RTree::bulk_load(indexed)
}

fn capped(violations: &[DrcViolation], cap: Option<usize>) -> bool {
    cap.is_some_and(|c| violations.len() >= c)
}

/// Manhattan distance between two non-overlapping rectangles.
/// Returns 0 if they touch or overlap.
#[inline]
fn rect_spacing(a: &Rect, b: &Rect) -> i32 {
    let dx = if a.x1 <= b.x0 {
        b.x0 - a.x1
    } else if b.x1 <= a.x0 {
        a.x0 - b.x1
    } else {
        0
    };

    let dy = if a.y1 <= b.y0 {
        b.y0 - a.y1
    } else if b.y1 <= a.y0 {
        a.y0 - b.y1
    } else {
        0
    };

    // Manhattan spacing for DRC is typically checked per-axis
    if dx > 0 && dy > 0 {
        dx.min(dy)
    } else {
        dx.max(dy)
    }
}

/// Run DRC checks on generated polygons using R-tree for scalable spacing checks.
/// Returns a list of violations (empty = clean).
///
/// Checks:
/// 1. Minimum width
/// 2. Maximum width (Cu layers)
/// 3. Minimum spacing (R-tree accelerated, O(n log n))
/// 4. Wide-metal spacing (Cu layers)
/// 5. Minimum area
/// 6. Metal density (SAT-accelerated window-based)
pub fn check_drc(rects: &[Rect], db_units_per_um: u32, drc: &DrcRules) -> Vec<DrcViolation> {
    check_drc_capped(rects, db_units_per_um, drc, None)
}

/// Like `check_drc` but stops after `max_violations` are found.
pub fn check_drc_capped(
    rects: &[Rect],
    db_units_per_um: u32,
    drc: &DrcRules,
    max_violations: Option<usize>,
) -> Vec<DrcViolation> {
    let mut violations = Vec::new();

    let min_w_dbu = um_to_dbu(drc.min_width, db_units_per_um);
    let min_s_dbu = um_to_dbu(drc.min_spacing, db_units_per_um);
    let min_area_dbu2 = if drc.min_area > 0.0 {
        let side = um_to_dbu(drc.min_area.sqrt(), db_units_per_um);
        (side as i64) * (side as i64)
    } else {
        0
    };
    let max_w_dbu = drc.max_width.map(|w| um_to_dbu(w, db_units_per_um));
    let wide_thresh_dbu = drc
        .wide_metal_threshold
        .map(|t| um_to_dbu(t, db_units_per_um));
    let wide_s_dbu = drc
        .wide_metal_spacing
        .map(|s| um_to_dbu(s, db_units_per_um));

    let use_parallel = max_violations.is_none() && rects.len() >= PARALLEL_RECT_THRESHOLD;

    // Check widths, max width, and area - zero-allocation iterator chains
    if use_parallel {
        let width_area_violations: Vec<DrcViolation> = rects
            .par_iter()
            .enumerate()
            .flat_map_iter(|(i, r)| {
                let idx = i as u32;
                let loc = (r.x0, r.y0);
                let width_v = (r.width() < min_w_dbu).then(|| DrcViolation {
                    rule: DrcRule::MinWidth,
                    rect_index: idx,
                    other_index: 0,
                    value: r.width() as i64,
                    limit: min_w_dbu as i64,
                    location: loc,
                });
                let height_v = (r.height() < min_w_dbu).then(|| DrcViolation {
                    rule: DrcRule::MinWidth,
                    rect_index: idx,
                    other_index: 0,
                    value: r.height() as i64,
                    limit: min_w_dbu as i64,
                    location: loc,
                });
                let max_w_v = max_w_dbu.and_then(|max_w| {
                    (r.width() > max_w || r.height() > max_w).then(|| DrcViolation {
                        rule: DrcRule::MaxWidth,
                        rect_index: idx,
                        other_index: 0,
                        value: r.width().max(r.height()) as i64,
                        limit: max_w as i64,
                        location: loc,
                    })
                });
                let area_v =
                    (min_area_dbu2 > 0 && r.area() < min_area_dbu2).then(|| DrcViolation {
                        rule: DrcRule::MinArea,
                        rect_index: idx,
                        other_index: 0,
                        value: r.area(),
                        limit: min_area_dbu2,
                        location: loc,
                    });
                width_v
                    .into_iter()
                    .chain(height_v)
                    .chain(max_w_v)
                    .chain(area_v)
            })
            .collect();
        violations.extend(width_area_violations);
    } else {
        for (i, r) in rects.iter().enumerate() {
            if capped(&violations, max_violations) {
                return violations;
            }
            let idx = i as u32;
            let loc = (r.x0, r.y0);
            if r.width() < min_w_dbu {
                violations.push(DrcViolation {
                    rule: DrcRule::MinWidth,
                    rect_index: idx,
                    other_index: 0,
                    value: r.width() as i64,
                    limit: min_w_dbu as i64,
                    location: loc,
                });
            }
            if r.height() < min_w_dbu {
                violations.push(DrcViolation {
                    rule: DrcRule::MinWidth,
                    rect_index: idx,
                    other_index: 0,
                    value: r.height() as i64,
                    limit: min_w_dbu as i64,
                    location: loc,
                });
            }
            if let Some(max_w) = max_w_dbu
                && (r.width() > max_w || r.height() > max_w)
            {
                violations.push(DrcViolation {
                    rule: DrcRule::MaxWidth,
                    rect_index: idx,
                    other_index: 0,
                    value: r.width().max(r.height()) as i64,
                    limit: max_w as i64,
                    location: loc,
                });
            }
            if min_area_dbu2 > 0 && r.area() < min_area_dbu2 {
                violations.push(DrcViolation {
                    rule: DrcRule::MinArea,
                    rect_index: idx,
                    other_index: 0,
                    value: r.area(),
                    limit: min_area_dbu2,
                    location: loc,
                });
            }
        }
    }

    // Build R-tree for spacing checks
    let tree = build_rtree(rects);

    // R-tree accelerated spacing check - zero-allocation iterator chains
    let search_margin = match wide_s_dbu {
        Some(ws) => min_s_dbu.max(ws),
        None => min_s_dbu,
    };

    if use_parallel {
        let spacing_violations: Vec<DrcViolation> = rects
            .par_iter()
            .enumerate()
            .flat_map_iter(|(i, r)| {
                let search_env = AABB::from_corners(
                    [r.x0 - search_margin, r.y0 - search_margin],
                    [r.x1 + search_margin, r.y1 + search_margin],
                );
                tree.locate_in_envelope_intersecting(&search_env)
                    .filter(move |neighbor| neighbor.index as usize > i)
                    .filter_map(move |neighbor| {
                        let dist = rect_spacing(r, &neighbor.rect);
                        if dist <= 0 {
                            return None;
                        }
                        let (effective_spacing, rule_kind) =
                            if let (Some(thresh), Some(ws)) = (wide_thresh_dbu, wide_s_dbu) {
                                let a_wide = r.width() >= thresh || r.height() >= thresh;
                                let b_wide = neighbor.rect.width() >= thresh
                                    || neighbor.rect.height() >= thresh;
                                if a_wide || b_wide {
                                    (ws, DrcRule::WideMetalSpacing)
                                } else {
                                    (min_s_dbu, DrcRule::MinSpacing)
                                }
                            } else {
                                (min_s_dbu, DrcRule::MinSpacing)
                            };
                        (dist < effective_spacing).then_some(DrcViolation {
                            rule: rule_kind,
                            rect_index: i as u32,
                            other_index: neighbor.index,
                            value: dist as i64,
                            limit: effective_spacing as i64,
                            location: (r.x0, r.y0),
                        })
                    })
            })
            .collect();
        violations.extend(spacing_violations);
    } else {
        for (i, r) in rects.iter().enumerate() {
            if capped(&violations, max_violations) {
                return violations;
            }
            let search_env = AABB::from_corners(
                [r.x0 - search_margin, r.y0 - search_margin],
                [r.x1 + search_margin, r.y1 + search_margin],
            );
            for neighbor in tree.locate_in_envelope_intersecting(&search_env) {
                if (neighbor.index as usize) <= i {
                    continue;
                }
                let dist = rect_spacing(r, &neighbor.rect);
                if dist <= 0 {
                    continue;
                }
                let (effective_spacing, rule_kind) =
                    if let (Some(thresh), Some(ws)) = (wide_thresh_dbu, wide_s_dbu) {
                        let a_wide = r.width() >= thresh || r.height() >= thresh;
                        let b_wide =
                            neighbor.rect.width() >= thresh || neighbor.rect.height() >= thresh;
                        if a_wide || b_wide {
                            (ws, DrcRule::WideMetalSpacing)
                        } else {
                            (min_s_dbu, DrcRule::MinSpacing)
                        }
                    } else {
                        (min_s_dbu, DrcRule::MinSpacing)
                    };
                if dist < effective_spacing {
                    violations.push(DrcViolation {
                        rule: rule_kind,
                        rect_index: i as u32,
                        other_index: neighbor.index,
                        value: dist as i64,
                        limit: effective_spacing as i64,
                        location: (r.x0, r.y0),
                    });
                }
            }
        }
    }

    // Density check using SAT (no longer needs R-tree)
    check_density(rects, db_units_per_um, drc, &mut violations, max_violations);

    violations
}

/// Check metal density using SAT (summed area table) for O(1) per-window queries.
///
/// Rasterizes rect metal area into a grid (cell = half-window step), builds a 2D prefix
/// sum, then queries each sliding window position in constant time.
fn check_density(
    rects: &[Rect],
    db_units_per_um: u32,
    drc: &DrcRules,
    violations: &mut Vec<DrcViolation>,
    max_violations: Option<usize>,
) {
    if drc.density_max >= 1.0 && drc.density_min <= 0.0 {
        return; // No meaningful density constraints
    }

    let bb = match bounding_box(rects) {
        Some(bb) => bb,
        None => return,
    };

    let window_dbu = um_to_dbu(drc.density_window_um, db_units_per_um);
    if window_dbu <= 0 {
        return;
    }

    let step = (window_dbu / 2).max(1);
    let window_area_i64 = window_dbu as i64 * window_dbu as i64;
    let max_metal = (drc.density_max * window_area_i64 as f64) as i64;
    let min_metal = (drc.density_min * window_area_i64 as f64) as i64;
    let density_max_permille = (drc.density_max * 1000.0) as i64;
    let density_min_permille = (drc.density_min * 1000.0) as i64;

    // Grid covers bounding box; each cell = step x step dbu
    let grid_x0 = bb.x0;
    let grid_y0 = bb.y0;
    let grid_w = ((bb.x1 - bb.x0) as usize).div_ceil(step as usize);
    let grid_h = ((bb.y1 - bb.y0) as usize).div_ceil(step as usize);

    if grid_w == 0 || grid_h == 0 {
        return;
    }

    // Rasterize: compute metal area per grid cell
    // Interior/border split: fully covered cells get step*step directly.
    let full_cell_area = step as i64 * step as i64;
    let mut grid = vec![0i64; grid_w * grid_h];
    for r in rects {
        let gx0 = ((r.x0 - grid_x0) / step).max(0) as usize;
        let gy0 = ((r.y0 - grid_y0) / step).max(0) as usize;
        let gx1 = (((r.x1 - grid_x0) + step - 1) / step).max(0) as usize;
        let gy1 = (((r.y1 - grid_y0) + step - 1) / step).max(0) as usize;
        let gx1 = gx1.min(grid_w);
        let gy1 = gy1.min(grid_h);

        // Interior range: cells fully covered by rect (no clipping needed)
        let igx0 = ((r.x0 - grid_x0 + step - 1) / step).max(0) as usize;
        let igy0 = ((r.y0 - grid_y0 + step - 1) / step).max(0) as usize;
        let igx1 = ((r.x1 - grid_x0) / step).max(0) as usize;
        let igy1 = ((r.y1 - grid_y0) / step).max(0) as usize;
        let igx1 = igx1.min(grid_w);
        let igy1 = igy1.min(grid_h);

        for gy in gy0..gy1 {
            let is_interior_y = gy >= igy0 && gy < igy1;
            for gx in gx0..gx1 {
                if is_interior_y && gx >= igx0 && gx < igx1 {
                    grid[gy * grid_w + gx] += full_cell_area;
                } else {
                    let cx0 = (grid_x0 + gx as i32 * step).max(r.x0);
                    let cy0 = (grid_y0 + gy as i32 * step).max(r.y0);
                    let cx1 = (grid_x0 + (gx as i32 + 1) * step).min(r.x1);
                    let cy1 = (grid_y0 + (gy as i32 + 1) * step).min(r.y1);
                    if cx1 > cx0 && cy1 > cy0 {
                        grid[gy * grid_w + gx] += (cx1 - cx0) as i64 * (cy1 - cy0) as i64;
                    }
                }
            }
        }
    }

    // Build SAT with dimensions (grid_w+1) x (grid_h+1)
    let sat_w = grid_w + 1;
    let mut sat = vec![0i64; sat_w * (grid_h + 1)];
    for gy in 0..grid_h {
        let mut row_sum = 0i64;
        for gx in 0..grid_w {
            row_sum += grid[gy * grid_w + gx];
            sat[(gy + 1) * sat_w + (gx + 1)] = row_sum + sat[gy * sat_w + (gx + 1)];
        }
    }

    // Window = window_dbu = 2 * step, so each window covers window_cells grid cells
    let window_cells = (window_dbu / step) as usize;

    // Collect all valid window positions
    let use_parallel = rects.len() >= PARALLEL_RECT_THRESHOLD && max_violations.is_none();

    if use_parallel {
        // Compute window grid dimensions for allocation-free parallel iteration
        let nx = ((bb.x1 - bb.x0 - window_dbu) / step + 1).max(0) as usize;
        let ny = ((bb.y1 - bb.y0 - window_dbu) / step + 1).max(0) as usize;

        let density_violations: Vec<DrcViolation> = (0..nx * ny)
            .into_par_iter()
            .flat_map_iter(|idx| {
                let ix = idx / ny;
                let iy = idx % ny;
                let wx = bb.x0 + ix as i32 * step;
                let wy = bb.y0 + iy as i32 * step;
                let gx0 = ((wx - grid_x0) / step) as usize;
                let gy0 = ((wy - grid_y0) / step) as usize;
                let gx1 = (gx0 + window_cells).min(grid_w);
                let gy1 = (gy0 + window_cells).min(grid_h);

                let metal_area =
                    sat[gy1 * sat_w + gx1] - sat[gy0 * sat_w + gx1] - sat[gy1 * sat_w + gx0]
                        + sat[gy0 * sat_w + gx0];

                let max_v = (metal_area > max_metal).then(|| DrcViolation {
                    rule: DrcRule::DensityMax,
                    rect_index: 0,
                    other_index: 0,
                    value: metal_area * 1000 / window_area_i64,
                    limit: density_max_permille,
                    location: (wx, wy),
                });
                let min_v = (min_metal > 0 && metal_area < min_metal).then(|| DrcViolation {
                    rule: DrcRule::DensityMin,
                    rect_index: 0,
                    other_index: 0,
                    value: metal_area * 1000 / window_area_i64,
                    limit: density_min_permille,
                    location: (wx, wy),
                });
                max_v.into_iter().chain(min_v)
            })
            .collect();
        violations.extend(density_violations);
    } else {
        let mut wx = bb.x0;
        while wx + window_dbu <= bb.x1 {
            let mut wy = bb.y0;
            while wy + window_dbu <= bb.y1 {
                if capped(violations, max_violations) {
                    return;
                }

                let gx0 = ((wx - grid_x0) / step) as usize;
                let gy0 = ((wy - grid_y0) / step) as usize;
                let gx1 = (gx0 + window_cells).min(grid_w);
                let gy1 = (gy0 + window_cells).min(grid_h);

                let metal_area =
                    sat[gy1 * sat_w + gx1] - sat[gy0 * sat_w + gx1] - sat[gy1 * sat_w + gx0]
                        + sat[gy0 * sat_w + gx0];

                if metal_area > max_metal {
                    let density_permille = metal_area * 1000 / window_area_i64;
                    violations.push(DrcViolation {
                        rule: DrcRule::DensityMax,
                        rect_index: 0,
                        other_index: 0,
                        value: density_permille,
                        limit: density_max_permille,
                        location: (wx, wy),
                    });
                }
                if min_metal > 0 && metal_area < min_metal {
                    let density_permille = metal_area * 1000 / window_area_i64;
                    violations.push(DrcViolation {
                        rule: DrcRule::DensityMin,
                        rect_index: 0,
                        other_index: 0,
                        value: density_permille,
                        limit: density_min_permille,
                        location: (wx, wy),
                    });
                }
                wy += step;
            }
            wx += step;
        }
    }
}

/// Check only density rules (skip width/spacing/area).
/// Much faster than full DRC - used in the density feedback loop.
pub fn check_density_only(
    rects: &[Rect],
    db_units_per_um: u32,
    drc: &DrcRules,
    max_violations: Option<usize>,
) -> Vec<DrcViolation> {
    let mut violations = Vec::new();
    if drc.density_max >= 1.0 && drc.density_min <= 0.0 {
        return violations;
    }
    check_density(rects, db_units_per_um, drc, &mut violations, max_violations);
    violations
}

/// Report DRC results to the log
pub fn report_drc(violations: &[DrcViolation]) {
    if violations.is_empty() {
        tracing::info!("DRC CLEAN - no violations found");
    } else {
        tracing::warn!("DRC VIOLATIONS: {} found", violations.len());
        for (i, v) in violations.iter().enumerate().take(20) {
            tracing::warn!("  [{}] {}: {}", i, v.rule, v);
        }
        if violations.len() > 20 {
            tracing::warn!("  ... and {} more", violations.len() - 20);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DBU_PER_UM: u32 = 1000;

    /// Minimal DRC rules for basic width/spacing/area tests (no wide-metal, no max_width).
    fn basic_rules() -> DrcRules {
        DrcRules {
            min_width: 1.0,
            min_spacing: 0.5,
            min_area: 2.0,
            density_min: 0.0,
            density_max: 1.0,
            density_window_um: 500.0,
            max_width: None,
            wide_metal_threshold: None,
            wide_metal_spacing: None,
        }
    }

    /// DRC rules with Cu-style wide-metal and max_width constraints.
    fn cu_rules() -> DrcRules {
        DrcRules {
            min_width: 0.300,
            min_spacing: 0.300,
            min_area: 0.240,
            density_min: 0.0,
            density_max: 0.77,
            density_window_um: 50.0,
            max_width: Some(10.0),
            wide_metal_threshold: Some(1.5),
            wide_metal_spacing: Some(0.400),
        }
    }

    #[test]
    fn test_width_violation() {
        let drc = basic_rules();
        let min_w = um_to_dbu(drc.min_width, DBU_PER_UM);
        let rects = vec![Rect::new(0, 0, min_w - 1, min_w)];
        let v = check_drc(&rects, DBU_PER_UM, &drc);
        assert!(v.iter().any(|v| v.rule == DrcRule::MinWidth));
    }

    #[test]
    fn test_spacing_violation() {
        let drc = basic_rules();
        let min_w = um_to_dbu(drc.min_width, DBU_PER_UM);
        let min_s = um_to_dbu(drc.min_spacing, DBU_PER_UM);
        let rects = vec![
            Rect::new(0, 0, min_w, min_w),
            Rect::new(min_w + min_s - 1, 0, min_w + min_s - 1 + min_w, min_w),
        ];
        let v = check_drc(&rects, DBU_PER_UM, &drc);
        assert!(v.iter().any(|v| v.rule == DrcRule::MinSpacing));
    }

    #[test]
    fn test_clean_rects() {
        let drc = basic_rules();
        let min_w = um_to_dbu(drc.min_width, DBU_PER_UM);
        let min_s = um_to_dbu(drc.min_spacing, DBU_PER_UM);
        let size = min_w * 2;
        let rects = vec![
            Rect::new(0, 0, size, size),
            Rect::new(size + min_s, 0, 2 * size + min_s, size),
        ];
        let v = check_drc(&rects, DBU_PER_UM, &drc);
        let non_density: Vec<_> = v
            .iter()
            .filter(|v| !matches!(v.rule, DrcRule::DensityMax | DrcRule::DensityMin))
            .collect();
        assert!(
            non_density.is_empty(),
            "Unexpected violations: {:?}",
            non_density
        );
    }

    #[test]
    fn test_large_rect_count_no_skip() {
        let drc = basic_rules();
        let min_w = um_to_dbu(drc.min_width, DBU_PER_UM);
        let min_s = um_to_dbu(drc.min_spacing, DBU_PER_UM);
        let size = min_w * 2;
        let pitch = size + min_s;
        let mut rects = Vec::new();
        let side = 110; // 110*110 = 12,100 rects
        for iy in 0..side {
            for ix in 0..side {
                rects.push(Rect::new(
                    ix * pitch,
                    iy * pitch,
                    ix * pitch + size,
                    iy * pitch + size,
                ));
            }
        }
        assert!(rects.len() > 10_000);
        let v = check_drc(&rects, DBU_PER_UM, &drc);
        let structural: Vec<_> = v
            .iter()
            .filter(|v| {
                matches!(
                    v.rule,
                    DrcRule::MinWidth | DrcRule::MinSpacing | DrcRule::MinArea
                )
            })
            .collect();
        assert!(
            structural.is_empty(),
            "Unexpected violations: {:?}",
            structural
        );
    }

    #[test]
    fn test_density_violation() {
        let drc = DrcRules {
            density_max: 0.77,
            density_window_um: 50.0,
            ..basic_rules()
        };
        let window_dbu = um_to_dbu(drc.density_window_um, DBU_PER_UM);
        let rects = vec![Rect::new(0, 0, window_dbu, window_dbu)];
        let v = check_drc(&rects, DBU_PER_UM, &drc);
        assert!(
            v.iter().any(|v| v.rule == DrcRule::DensityMax),
            "Expected density violation for 100% fill"
        );
    }

    #[test]
    fn test_max_width_violation() {
        let drc = cu_rules();
        let max_w_dbu = um_to_dbu(drc.max_width.unwrap(), DBU_PER_UM);
        let rects = vec![Rect::new(0, 0, max_w_dbu + 100, 500)];
        let v = check_drc(&rects, DBU_PER_UM, &drc);
        assert!(
            v.iter().any(|v| v.rule == DrcRule::MaxWidth),
            "Expected max_width violation for oversized rect"
        );
    }

    #[test]
    fn test_wide_metal_spacing_violation() {
        let drc = cu_rules();
        let thresh_dbu = um_to_dbu(drc.wide_metal_threshold.unwrap(), DBU_PER_UM);
        let min_s_dbu = um_to_dbu(drc.min_spacing, DBU_PER_UM);
        let wide_s_dbu = um_to_dbu(drc.wide_metal_spacing.unwrap(), DBU_PER_UM);
        // Gap between min_spacing and wide_metal_spacing
        let gap = (min_s_dbu + wide_s_dbu) / 2;
        let rects = vec![
            Rect::new(0, 0, thresh_dbu, thresh_dbu),
            Rect::new(thresh_dbu + gap, 0, 2 * thresh_dbu + gap, thresh_dbu),
        ];
        let v = check_drc(&rects, DBU_PER_UM, &drc);
        assert!(
            v.iter().any(|v| v.rule == DrcRule::WideMetalSpacing),
            "Expected wide_metal_spacing violation, got: {:?}",
            v
        );
    }

    #[test]
    fn test_wide_metal_spacing_clean() {
        let drc = cu_rules();
        let min_s_dbu = um_to_dbu(drc.min_spacing, DBU_PER_UM);
        let min_w_dbu = um_to_dbu(drc.min_width, DBU_PER_UM);
        // Two small rects (below threshold) at min_spacing - should be clean
        let size = min_w_dbu * 2;
        let rects = vec![
            Rect::new(0, 0, size, size),
            Rect::new(size + min_s_dbu, 0, 2 * size + min_s_dbu, size),
        ];
        let v = check_drc(&rects, DBU_PER_UM, &drc);
        let spacing_violations: Vec<_> = v
            .iter()
            .filter(|v| matches!(v.rule, DrcRule::MinSpacing | DrcRule::WideMetalSpacing))
            .collect();
        assert!(
            spacing_violations.is_empty(),
            "Unexpected spacing violations: {:?}",
            spacing_violations
        );
    }

    #[test]
    fn test_early_exit_cap() {
        let drc = basic_rules();
        // Create many rects that violate min_width
        let rects: Vec<Rect> = (0..100)
            .map(|i| Rect::new(i * 100, 0, i * 100 + 1, 1))
            .collect();
        let uncapped = check_drc(&rects, DBU_PER_UM, &drc);
        let capped = check_drc_capped(&rects, DBU_PER_UM, &drc, Some(10));
        // Capped should have far fewer violations than uncapped
        assert!(
            capped.len() <= 15,
            "cap should limit violations, got {}",
            capped.len()
        );
        assert!(
            capped.len() < uncapped.len(),
            "capped ({}) should be less than uncapped ({})",
            capped.len(),
            uncapped.len()
        );
    }

    #[test]
    fn test_check_density_only() {
        let drc = DrcRules {
            density_max: 0.77,
            density_window_um: 50.0,
            ..basic_rules()
        };
        let window_dbu = um_to_dbu(drc.density_window_um, DBU_PER_UM);
        // 100% fill - should trigger density violation
        let rects = vec![Rect::new(0, 0, window_dbu, window_dbu)];
        let v = check_density_only(&rects, DBU_PER_UM, &drc, None);
        assert!(
            v.iter().any(|v| v.rule == DrcRule::DensityMax),
            "Expected density violation for 100% fill"
        );
        // All violations should be density-related
        assert!(
            v.iter()
                .all(|v| matches!(v.rule, DrcRule::DensityMax | DrcRule::DensityMin)),
            "check_density_only should only return density violations"
        );
    }

    #[test]
    fn test_check_density_only_clean() {
        let drc = DrcRules {
            density_max: 0.77,
            density_window_um: 50.0,
            ..basic_rules()
        };
        let window_dbu = um_to_dbu(drc.density_window_um, DBU_PER_UM);
        // 50% fill - should be clean
        let half = window_dbu / 2;
        let rects = vec![Rect::new(0, 0, half, window_dbu)];
        let v = check_density_only(&rects, DBU_PER_UM, &drc, None);
        assert!(
            v.is_empty(),
            "50% fill should pass 77% density limit, got {:?}",
            v
        );
    }
}
