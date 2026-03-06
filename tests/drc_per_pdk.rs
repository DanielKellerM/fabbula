//! Integration tests: run DRC checks against every built-in PDK.
//!
//! These tests verify that:
//! - Clean geometry passes DRC for each PDK's primary (and alt) rules
//! - Known-bad geometry triggers the expected violations
//! - The checker works at each PDK's specific db_units_per_um scale
//!
//! Adding a new PDK? It will automatically be covered by these tests.

use fabbula::drc::{DrcRule, DrcViolation, check_drc};
use fabbula::pdk::{DrcRules, PdkConfig};
use fabbula::polygon::Rect;

/// Helper: convert um to dbu for a given PDK.
fn dbu(pdk: &PdkConfig, um: f64) -> i32 {
    pdk.um_to_dbu(um)
}

/// Collect all (pdk, drc_rules, label) combos to test.
fn all_pdk_rule_sets() -> Vec<(PdkConfig, DrcRules, String)> {
    let mut sets = Vec::new();
    for name in PdkConfig::list_builtins() {
        let pdk = PdkConfig::builtin(name).unwrap();
        sets.push((pdk.clone(), pdk.drc.clone(), format!("{}/primary", name)));
        if let Some(alt) = &pdk.drc_alt {
            sets.push((pdk.clone(), alt.clone(), format!("{}/alt", name)));
        }
    }
    sets
}

fn has_rule(violations: &[DrcViolation], rule: DrcRule) -> bool {
    violations.iter().any(|v| v.rule == rule)
}

fn structural_violations(violations: &[DrcViolation]) -> Vec<&DrcViolation> {
    violations
        .iter()
        .filter(|v| !matches!(v.rule, DrcRule::DensityMax | DrcRule::DensityMin))
        .collect()
}

// ---------------------------------------------------------------------------
// Clean geometry: properly-sized rects at proper spacing should pass
// ---------------------------------------------------------------------------

#[test]
fn clean_rects_all_pdks() {
    for (pdk, drc, label) in all_pdk_rule_sets() {
        let u = pdk.pdk.db_units_per_um;
        let min_w = dbu(&pdk, drc.min_width);
        let eff_s = dbu(&pdk, drc.effective_spacing());
        // Size must satisfy both min_width and min_area
        let min_area_dbu2 = (drc.min_area * (u as f64 * u as f64)) as i32;
        let min_side_for_area = if min_area_dbu2 > 0 {
            ((min_area_dbu2 as f64).sqrt().ceil() as i32).max(min_w)
        } else {
            min_w
        };
        let size = min_side_for_area.max(min_w * 2);
        let rects = vec![
            Rect::new(0, 0, size, size),
            Rect::new(size + eff_s, 0, 2 * size + eff_s, size),
        ];
        let v = check_drc(&rects, u, &drc);
        let bad = structural_violations(&v);
        assert!(
            bad.is_empty(),
            "[{}] unexpected violations: {:?}",
            label,
            bad
        );
    }
}

// ---------------------------------------------------------------------------
// Min width violation
// ---------------------------------------------------------------------------

#[test]
fn min_width_violation_all_pdks() {
    for (pdk, drc, label) in all_pdk_rule_sets() {
        let u = pdk.pdk.db_units_per_um;
        let min_w = dbu(&pdk, drc.min_width);
        // One dimension below min_width
        let rects = vec![Rect::new(0, 0, min_w - 1, min_w * 2)];
        let v = check_drc(&rects, u, &drc);
        assert!(
            has_rule(&v, DrcRule::MinWidth),
            "[{}] expected min_width violation",
            label
        );
    }
}

// ---------------------------------------------------------------------------
// Min spacing violation
// ---------------------------------------------------------------------------

#[test]
fn min_spacing_violation_all_pdks() {
    for (pdk, drc, label) in all_pdk_rule_sets() {
        let u = pdk.pdk.db_units_per_um;
        let min_w = dbu(&pdk, drc.min_width);
        let min_s = dbu(&pdk, drc.min_spacing);
        let size = min_w * 2;
        // Gap = min_spacing - 1 dbu
        let rects = vec![
            Rect::new(0, 0, size, size),
            Rect::new(size + min_s - 1, 0, 2 * size + min_s - 1, size),
        ];
        let v = check_drc(&rects, u, &drc);
        assert!(
            has_rule(&v, DrcRule::MinSpacing) || has_rule(&v, DrcRule::WideMetalSpacing),
            "[{}] expected spacing violation",
            label
        );
    }
}

// ---------------------------------------------------------------------------
// Max width violation (only PDKs that define it)
// ---------------------------------------------------------------------------

#[test]
fn max_width_violation_where_defined() {
    for (pdk, drc, label) in all_pdk_rule_sets() {
        let u = pdk.pdk.db_units_per_um;
        if let Some(max_w) = drc.max_width {
            let max_w_dbu = dbu(&pdk, max_w);
            let rects = vec![Rect::new(0, 0, max_w_dbu + 100, 500)];
            let v = check_drc(&rects, u, &drc);
            assert!(
                has_rule(&v, DrcRule::MaxWidth),
                "[{}] expected max_width violation",
                label
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Wide-metal spacing violation (only PDKs that define it)
// ---------------------------------------------------------------------------

#[test]
fn wide_metal_spacing_violation_where_defined() {
    for (pdk, drc, label) in all_pdk_rule_sets() {
        let u = pdk.pdk.db_units_per_um;
        if let (Some(thresh), Some(wide_s)) = (drc.wide_metal_threshold, drc.wide_metal_spacing) {
            let thresh_dbu = dbu(&pdk, thresh);
            let min_s_dbu = dbu(&pdk, drc.min_spacing);
            let wide_s_dbu = dbu(&pdk, wide_s);
            // Gap between min_spacing and wide_metal_spacing
            let gap = (min_s_dbu + wide_s_dbu) / 2;
            let rects = vec![
                Rect::new(0, 0, thresh_dbu, thresh_dbu),
                Rect::new(thresh_dbu + gap, 0, 2 * thresh_dbu + gap, thresh_dbu),
            ];
            let v = check_drc(&rects, u, &drc);
            assert!(
                has_rule(&v, DrcRule::WideMetalSpacing),
                "[{}] expected wide_metal_spacing violation",
                label
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Wide-metal spacing clean for small rects (only PDKs that define it)
// ---------------------------------------------------------------------------

#[test]
fn wide_metal_spacing_clean_for_small_rects() {
    for (pdk, drc, label) in all_pdk_rule_sets() {
        let u = pdk.pdk.db_units_per_um;
        if let Some(thresh) = drc.wide_metal_threshold
            && drc.wide_metal_spacing.is_some()
        {
            let min_w_dbu = dbu(&pdk, drc.min_width);
            let min_s_dbu = dbu(&pdk, drc.min_spacing);
            let thresh_dbu = dbu(&pdk, thresh);
            // Rects must be below the threshold in both dimensions
            let size = min_w_dbu.min(thresh_dbu - 1);
            if size < min_w_dbu {
                // Can't make rects that are both >= min_width and < threshold, skip
                continue;
            }
            let rects = vec![
                Rect::new(0, 0, size, size),
                Rect::new(size + min_s_dbu, 0, 2 * size + min_s_dbu, size),
            ];
            let v = check_drc(&rects, u, &drc);
            let spacing: Vec<_> = v
                .iter()
                .filter(|v| matches!(v.rule, DrcRule::MinSpacing | DrcRule::WideMetalSpacing))
                .collect();
            assert!(
                spacing.is_empty(),
                "[{}] unexpected spacing violations for small rects: {:?}",
                label,
                spacing
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Density violation (only PDKs with density_max < 1.0)
// ---------------------------------------------------------------------------

#[test]
fn density_violation_where_constrained() {
    for (pdk, drc, label) in all_pdk_rule_sets() {
        let u = pdk.pdk.db_units_per_um;
        if drc.density_max < 1.0 {
            let window_dbu = dbu(&pdk, drc.density_window_um);
            // 100% fill in one density window
            let rects = vec![Rect::new(0, 0, window_dbu, window_dbu)];
            let v = check_drc(&rects, u, &drc);
            assert!(
                has_rule(&v, DrcRule::DensityMax),
                "[{}] expected density_max violation for 100% fill",
                label
            );
        }
    }
}
