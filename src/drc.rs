use crate::pdk::{um_to_dbu, DrcRules};
use crate::polygon::{bounding_box, Rect};
use rstar::{RTree, RTreeObject, AABB};

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

/// A DRC violation
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrcViolation {
    pub rule: DrcRule,
    pub message: String,
    pub location: Option<(i32, i32)>,
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

    // Check widths, max width, and area
    for (i, r) in rects.iter().enumerate() {
        if r.width() < min_w_dbu {
            violations.push(DrcViolation {
                rule: DrcRule::MinWidth,
                message: format!("Rect {} width {} dbu < min {} dbu", i, r.width(), min_w_dbu),
                location: Some((r.x0, r.y0)),
            });
        }
        if r.height() < min_w_dbu {
            violations.push(DrcViolation {
                rule: DrcRule::MinWidth,
                message: format!(
                    "Rect {} height {} dbu < min {} dbu",
                    i,
                    r.height(),
                    min_w_dbu
                ),
                location: Some((r.x0, r.y0)),
            });
        }

        if let Some(max_w) = max_w_dbu {
            if r.width() > max_w || r.height() > max_w {
                violations.push(DrcViolation {
                    rule: DrcRule::MaxWidth,
                    message: format!(
                        "Rect {} dimension {}x{} dbu exceeds max {} dbu",
                        i,
                        r.width(),
                        r.height(),
                        max_w
                    ),
                    location: Some((r.x0, r.y0)),
                });
            }
        }

        if min_area_dbu2 > 0 && r.area() < min_area_dbu2 {
            violations.push(DrcViolation {
                rule: DrcRule::MinArea,
                message: format!(
                    "Rect {} area {} dbu^2 < min {} dbu^2",
                    i,
                    r.area(),
                    min_area_dbu2
                ),
                location: Some((r.x0, r.y0)),
            });
        }
    }

    // R-tree accelerated spacing check
    let indexed: Vec<IndexedRect> = rects
        .iter()
        .enumerate()
        .map(|(i, r)| IndexedRect { index: i, rect: *r })
        .collect();
    let tree = RTree::bulk_load(indexed);

    // Search envelope must be large enough for wide-metal spacing too
    let search_margin = match wide_s_dbu {
        Some(ws) => min_s_dbu.max(ws),
        None => min_s_dbu,
    };

    for (i, r) in rects.iter().enumerate() {
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

            // Determine effective spacing requirement
            let (effective_spacing, rule_kind) = if let (Some(thresh), Some(ws)) =
                (wide_thresh_dbu, wide_s_dbu)
            {
                let a_wide = r.width() >= thresh || r.height() >= thresh;
                let b_wide = neighbor.rect.width() >= thresh || neighbor.rect.height() >= thresh;
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
                    message: format!(
                        "Rects {}-{} spacing {} dbu < min {} dbu",
                        i, neighbor.index, dist, effective_spacing
                    ),
                    location: Some((r.x0, r.y0)),
                });
            }
        }
    }

    // Density check
    check_density(rects, db_units_per_um, drc, &mut violations);

    violations
}

/// Check metal density using a sliding window approach with R-tree acceleration.
fn check_density(
    rects: &[Rect],
    db_units_per_um: u32,
    drc: &DrcRules,
    violations: &mut Vec<DrcViolation>,
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

    let indexed: Vec<IndexedRect> = rects
        .iter()
        .enumerate()
        .map(|(i, r)| IndexedRect { index: i, rect: *r })
        .collect();
    let tree = RTree::bulk_load(indexed);

    let window_area = window_dbu as f64 * window_dbu as f64;
    // Step by half-window for reasonable coverage without being too slow
    let step = window_dbu / 2;
    let step = step.max(1);

    let mut wx = bb.x0;
    while wx + window_dbu <= bb.x1 {
        let mut wy = bb.y0;
        while wy + window_dbu <= bb.y1 {
            let search_env = AABB::from_corners([wx, wy], [wx + window_dbu, wy + window_dbu]);
            let mut metal_area: f64 = 0.0;
            for ir in tree.locate_in_envelope_intersecting(&search_env) {
                // Clip rect to window
                let cx0 = ir.rect.x0.max(wx);
                let cy0 = ir.rect.y0.max(wy);
                let cx1 = ir.rect.x1.min(wx + window_dbu);
                let cy1 = ir.rect.y1.min(wy + window_dbu);
                if cx1 > cx0 && cy1 > cy0 {
                    metal_area += (cx1 - cx0) as f64 * (cy1 - cy0) as f64;
                }
            }
            let density = metal_area / window_area;
            if density > drc.density_max {
                violations.push(DrcViolation {
                    rule: DrcRule::DensityMax,
                    message: format!(
                        "Density {:.1}% exceeds max {:.1}% in window at ({}, {})",
                        density * 100.0,
                        drc.density_max * 100.0,
                        wx,
                        wy
                    ),
                    location: Some((wx, wy)),
                });
            }
            if drc.density_min > 0.0 && density < drc.density_min {
                violations.push(DrcViolation {
                    rule: DrcRule::DensityMin,
                    message: format!(
                        "Density {:.1}% below min {:.1}% in window at ({}, {})",
                        density * 100.0,
                        drc.density_min * 100.0,
                        wx,
                        wy
                    ),
                    location: Some((wx, wy)),
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

/// Report DRC results to the log
pub fn report_drc(violations: &[DrcViolation]) {
    if violations.is_empty() {
        tracing::info!("DRC CLEAN - no violations found");
    } else {
        tracing::warn!("DRC VIOLATIONS: {} found", violations.len());
        for (i, v) in violations.iter().enumerate().take(20) {
            tracing::warn!("  [{}] {}: {}", i, v.rule, v.message);
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
}
