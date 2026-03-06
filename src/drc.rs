use crate::pdk::{um_to_dbu, DrcRules};
use crate::polygon::{bounding_box, Rect};
use rayon::prelude::*;
use rstar::{RTree, RTreeObject, AABB};

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
    index: usize,
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
        .map(|(i, r)| IndexedRect { index: i, rect: *r })
        .collect();
    RTree::bulk_load(indexed)
}

fn capped(violations: &[DrcViolation], cap: Option<usize>) -> bool {
    cap.is_some_and(|c| violations.len() >= c)
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
/// 6. Metal density (window-based)
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

    // Check widths, max width, and area
    if use_parallel {
        let width_area_violations: Vec<DrcViolation> = rects
            .par_iter()
            .enumerate()
            .flat_map_iter(|(i, r)| {
                let mut local = Vec::new();
                if r.width() < min_w_dbu {
                    local.push(DrcViolation {
                        rule: DrcRule::MinWidth,
                        rect_index: i as u32,
                        other_index: 0,
                        value: r.width() as i64,
                        limit: min_w_dbu as i64,
                        location: (r.x0, r.y0),
                    });
                }
                if r.height() < min_w_dbu {
                    local.push(DrcViolation {
                        rule: DrcRule::MinWidth,
                        rect_index: i as u32,
                        other_index: 0,
                        value: r.height() as i64,
                        limit: min_w_dbu as i64,
                        location: (r.x0, r.y0),
                    });
                }
                if let Some(max_w) = max_w_dbu {
                    if r.width() > max_w || r.height() > max_w {
                        let dim = r.width().max(r.height());
                        local.push(DrcViolation {
                            rule: DrcRule::MaxWidth,
                            rect_index: i as u32,
                            other_index: 0,
                            value: dim as i64,
                            limit: max_w as i64,
                            location: (r.x0, r.y0),
                        });
                    }
                }
                if min_area_dbu2 > 0 && r.area() < min_area_dbu2 {
                    local.push(DrcViolation {
                        rule: DrcRule::MinArea,
                        rect_index: i as u32,
                        other_index: 0,
                        value: r.area(),
                        limit: min_area_dbu2,
                        location: (r.x0, r.y0),
                    });
                }
                local.into_iter()
            })
            .collect();
        violations.extend(width_area_violations);
    } else {
        for (i, r) in rects.iter().enumerate() {
            if capped(&violations, max_violations) {
                return violations;
            }
            if r.width() < min_w_dbu {
                violations.push(DrcViolation {
                    rule: DrcRule::MinWidth,
                    rect_index: i as u32,
                    other_index: 0,
                    value: r.width() as i64,
                    limit: min_w_dbu as i64,
                    location: (r.x0, r.y0),
                });
            }
            if r.height() < min_w_dbu {
                violations.push(DrcViolation {
                    rule: DrcRule::MinWidth,
                    rect_index: i as u32,
                    other_index: 0,
                    value: r.height() as i64,
                    limit: min_w_dbu as i64,
                    location: (r.x0, r.y0),
                });
            }
            if let Some(max_w) = max_w_dbu {
                if r.width() > max_w || r.height() > max_w {
                    let dim = r.width().max(r.height());
                    violations.push(DrcViolation {
                        rule: DrcRule::MaxWidth,
                        rect_index: i as u32,
                        other_index: 0,
                        value: dim as i64,
                        limit: max_w as i64,
                        location: (r.x0, r.y0),
                    });
                }
            }
            if min_area_dbu2 > 0 && r.area() < min_area_dbu2 {
                violations.push(DrcViolation {
                    rule: DrcRule::MinArea,
                    rect_index: i as u32,
                    other_index: 0,
                    value: r.area(),
                    limit: min_area_dbu2,
                    location: (r.x0, r.y0),
                });
            }
        }
    }

    // Build R-tree once, share between spacing and density checks
    let tree = build_rtree(rects);

    // R-tree accelerated spacing check
    let search_margin = match wide_s_dbu {
        Some(ws) => min_s_dbu.max(ws),
        None => min_s_dbu,
    };

    if use_parallel {
        let spacing_violations: Vec<DrcViolation> = rects
            .par_iter()
            .enumerate()
            .flat_map_iter(|(i, r)| {
                let mut local = Vec::new();
                let search_env = AABB::from_corners(
                    [r.x0 - search_margin, r.y0 - search_margin],
                    [r.x1 + search_margin, r.y1 + search_margin],
                );
                for neighbor in tree.locate_in_envelope_intersecting(&search_env) {
                    if neighbor.index <= i {
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
                        local.push(DrcViolation {
                            rule: rule_kind,
                            rect_index: i as u32,
                            other_index: neighbor.index as u32,
                            value: dist as i64,
                            limit: effective_spacing as i64,
                            location: (r.x0, r.y0),
                        });
                    }
                }
                local.into_iter()
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
                if neighbor.index <= i {
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
                        other_index: neighbor.index as u32,
                        value: dist as i64,
                        limit: effective_spacing as i64,
                        location: (r.x0, r.y0),
                    });
                }
            }
        }
    }

    // Density check - reuse the same R-tree
    check_density(
        &tree,
        rects,
        db_units_per_um,
        drc,
        &mut violations,
        max_violations,
    );

    violations
}

/// Check metal density using a sliding window approach, reusing the shared R-tree.
/// Uses integer-only arithmetic in the inner loop.
fn check_density(
    tree: &RTree<IndexedRect>,
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

    // Pre-compute integer thresholds (permille precision)
    let window_area_i64 = window_dbu as i64 * window_dbu as i64;
    let max_metal = (drc.density_max * window_area_i64 as f64) as i64;
    let min_metal = (drc.density_min * window_area_i64 as f64) as i64;
    let density_max_permille = (drc.density_max * 1000.0) as i64;
    let density_min_permille = (drc.density_min * 1000.0) as i64;

    // Step by half-window for reasonable coverage without being too slow
    let step = (window_dbu / 2).max(1);

    let mut wx = bb.x0;
    while wx + window_dbu <= bb.x1 {
        let mut wy = bb.y0;
        while wy + window_dbu <= bb.y1 {
            if capped(violations, max_violations) {
                return;
            }

            let search_env = AABB::from_corners([wx, wy], [wx + window_dbu, wy + window_dbu]);
            let mut metal_area: i64 = 0;
            for ir in tree.locate_in_envelope_intersecting(&search_env) {
                // Clip rect to window - pure integer
                let cx0 = ir.rect.x0.max(wx);
                let cy0 = ir.rect.y0.max(wy);
                let cx1 = ir.rect.x1.min(wx + window_dbu);
                let cy1 = ir.rect.y1.min(wy + window_dbu);
                if cx1 > cx0 && cy1 > cy0 {
                    metal_area += (cx1 - cx0) as i64 * (cy1 - cy0) as i64;
                }
            }

            if metal_area > max_metal {
                // Convert to permille only for the violation record
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

/// Manhattan distance between two non-overlapping rectangles.
/// Returns 0 if they touch or overlap.
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
        // Rects are diagonal - return the smaller axis distance
        dx.min(dy)
    } else {
        dx.max(dy)
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
    let tree = build_rtree(rects);
    check_density(
        &tree,
        rects,
        db_units_per_um,
        drc,
        &mut violations,
        max_violations,
    );
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
            min_enclosed_area: 0.0,
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
            min_enclosed_area: 0.0,
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
