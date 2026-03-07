// Copyright 2026 Daniel Keller <daniel.keller.m@gmail.com>
// Licensed under the Apache License, Version 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Generation pipeline with density enforcement feedback loop.
//!
//! Provides [`generate_layer_polygons`] which combines bitmap-level density
//! pre-enforcement with a closed-loop density correction pass.

use crate::artwork::{ArtworkBitmap, enforce_density, enforce_density_region};
use crate::drc::{DrcRule, check_density_only};
use crate::pdk::{DrcRules, PdkConfig};
use crate::polygon::{PixelPlacement, PolygonStrategy, Rect, generate_polygons};
use anyhow::Result;

/// Generate polygons with a closed-loop density enforcement.
///
/// After the initial bitmap-level density pre-pass, generates polygons and checks
/// for density violations. If any are found, maps the violating window back to
/// bitmap pixel coordinates and applies targeted density enforcement, then retries.
fn generate_with_density_loop(
    bitmap: &mut ArtworkBitmap,
    pdk: &PdkConfig,
    drc_rules: &DrcRules,
    strategy: PolygonStrategy,
    placement: PixelPlacement,
    max_retries: u32,
) -> Result<Vec<Rect>> {
    let touching = placement == PixelPlacement::Touching;
    let min_w_um = pdk.snap_to_grid(drc_rules.min_width);
    let eff_s_um = pdk.snap_to_grid(drc_rules.effective_spacing());
    let pitch_um = if touching {
        min_w_um
    } else {
        min_w_um + eff_s_um
    };
    let pitch_dbu = pdk.um_to_dbu(pitch_um).0;

    let mut best_rects = generate_polygons(bitmap, pdk, drc_rules, strategy, placement)?;

    if drc_rules.density_max >= 1.0 {
        return Ok(best_rects);
    }

    for attempt in 0..max_retries {
        let violations =
            check_density_only(&best_rects, pdk.pdk.db_units_per_um, drc_rules, Some(1));
        if violations.is_empty() {
            return Ok(best_rects);
        }

        let density_violations =
            check_density_only(&best_rects, pdk.pdk.db_units_per_um, drc_rules, None);
        let mut total_cleared = 0usize;

        for v in &density_violations {
            if v.rule != DrcRule::DensityMax {
                continue;
            }
            let (wx_dbu, wy_dbu) = (v.location.x.0, v.location.y.0);
            let window_dbu = pdk.um_to_dbu(drc_rules.density_window_um).0;

            let px_start = (wx_dbu / pitch_dbu).max(0) as u32;
            let px_end_dbu = wx_dbu + window_dbu;
            let px_end = ((px_end_dbu + pitch_dbu - 1) / pitch_dbu).max(0) as u32;

            let py_end_phys = wy_dbu;
            let py_start_phys = wy_dbu + window_dbu;
            let bh = bitmap.height as i32;
            let py_start = (bh - 1 - (py_start_phys / pitch_dbu)).max(0) as u32;
            let py_end = (bh - (py_end_phys / pitch_dbu)).max(0) as u32;

            let rw = px_end.saturating_sub(px_start).min(bitmap.width - px_start);
            let rh = py_end
                .saturating_sub(py_start)
                .min(bitmap.height - py_start);

            if rw > 0 && rh > 0 {
                // Target 5% below density_max to provide margin for window
                // alignment differences between bitmap-level and polygon-level checks.
                const DENSITY_MARGIN: f64 = 0.95;
                let tight_max = drc_rules.density_max * DENSITY_MARGIN;
                total_cleared +=
                    enforce_density_region(bitmap, tight_max, px_start, py_start, rw, rh);
            }
        }

        if total_cleared == 0 {
            tracing::warn!(
                "Density loop attempt {}: no pixels could be cleared, stopping",
                attempt + 1
            );
            break;
        }

        tracing::info!(
            "Density loop attempt {}: cleared {} pixels, regenerating polygons",
            attempt + 1,
            total_cleared
        );

        best_rects = generate_polygons(bitmap, pdk, drc_rules, strategy, placement)?;
    }

    Ok(best_rects)
}

/// Run density pre-pass enforcement for a given DRC rule set.
fn density_prepass(
    bitmap: &mut ArtworkBitmap,
    pdk: &PdkConfig,
    drc: &DrcRules,
    placement: PixelPlacement,
) {
    let touching = placement == PixelPlacement::Touching;
    let min_w_um = pdk.snap_to_grid(drc.min_width);
    let eff_s_um = pdk.snap_to_grid(drc.effective_spacing());
    let pitch_um = if touching {
        min_w_um
    } else {
        min_w_um + eff_s_um
    };
    let window_px = (drc.density_window_um / pitch_um).floor() as u32;
    if window_px == 0 {
        tracing::warn!(
            "Density window ({:.1} um) is smaller than pixel pitch ({:.1} um), \
             skipping density enforcement",
            drc.density_window_um,
            pitch_um
        );
    }
    if window_px > 0 && bitmap.density() > drc.density_max {
        let cleared = enforce_density(bitmap, drc.density_max, window_px);
        if cleared > 0 {
            tracing::info!("Density enforcement: cleared {} pixels", cleared);
        }
    }
}

/// Generate polygons for a single layer with optional density enforcement.
///
/// When `density_enforce` is true and density violations remain after the
/// feedback loop, returns an error unless `force` is true.
pub fn generate_layer_polygons(
    bitmap: &mut ArtworkBitmap,
    pdk: &PdkConfig,
    drc: &DrcRules,
    strategy: PolygonStrategy,
    placement: PixelPlacement,
    density_enforce: bool,
    force: bool,
) -> Result<Vec<Rect>> {
    if density_enforce && drc.density_max < 1.0 {
        density_prepass(bitmap, pdk, drc, placement);
        let rects = generate_with_density_loop(bitmap, pdk, drc, strategy, placement, 3)?;
        let final_violations = check_density_only(&rects, pdk.pdk.db_units_per_um, drc, Some(1));
        if !final_violations.is_empty() && !force {
            anyhow::bail!(
                "Density enforcement did not converge: {} violations remaining. \
                 Use --force to continue despite density violations.",
                final_violations.len()
            );
        }
        Ok(rects)
    } else {
        generate_polygons(bitmap, pdk, drc, strategy, placement)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artwork::ArtworkBitmap;
    use crate::pdk::PdkConfig;
    use crate::polygon::{PixelPlacement, PolygonStrategy};

    /// Create a bitmap with horizontal stripes of `run` ON pixels followed by
    /// `run` OFF pixels. This produces wide merged rects that satisfy min_area.
    fn make_striped(width: u32, height: u32, run: u32) -> ArtworkBitmap {
        let total = (width * height) as usize;
        let mut bools = vec![false; total];
        for y in 0..height {
            for x in 0..width {
                // Alternate ON/OFF in groups of `run` pixels
                if (x / run).is_multiple_of(2) {
                    bools[(y * width + x) as usize] = true;
                }
            }
        }
        ArtworkBitmap::from_bools(width, height, &bools)
    }

    /// Create a fully-filled bitmap (100% density).
    fn make_filled(width: u32, height: u32) -> ArtworkBitmap {
        let total = (width * height) as usize;
        ArtworkBitmap::from_bools(width, height, &vec![true; total])
    }

    #[test]
    fn generate_layer_polygons_basic() {
        let pdk = PdkConfig::builtin("sky130").unwrap();
        let drc = &pdk.drc;
        // Use touching placement so adjacent ON pixels merge into large rects
        // that satisfy min_area. A 16x16 solid block produces one big rect.
        let mut bitmap = make_filled(16, 16);
        let rects = generate_layer_polygons(
            &mut bitmap,
            &pdk,
            drc,
            PolygonStrategy::RowMerge,
            PixelPlacement::Touching,
            false,
            false,
        )
        .unwrap();
        assert!(
            !rects.is_empty(),
            "Should produce non-empty rects from filled bitmap with Touching placement"
        );
    }

    #[test]
    fn generate_layer_polygons_density_enforce_false() {
        let pdk = PdkConfig::builtin("sky130").unwrap();
        // Use the alt DRC rules which have density_max < 1.0
        let drc = pdk.drc_alt.as_ref().unwrap();
        // Striped pattern with runs of 4 ON pixels - produces merged rects
        // that satisfy met4's min_area (0.24 um^2). Each run of 4 pixels at
        // 0.3 um width = 1.2 um wide, area = 1.2 * 0.3 = 0.36 > 0.24.
        let mut bitmap = make_striped(16, 16, 4);
        // With density_enforce=false, it should skip the density loop entirely
        let rects = generate_layer_polygons(
            &mut bitmap,
            &pdk,
            drc,
            PolygonStrategy::RowMerge,
            PixelPlacement::Touching,
            false,
            false,
        )
        .unwrap();
        assert!(
            !rects.is_empty(),
            "Should produce rects without density enforcement"
        );
    }

    #[test]
    fn generate_layer_polygons_density_enforce_true() {
        let pdk = PdkConfig::builtin("sky130").unwrap();
        // Use alt DRC rules (density_max = 0.77)
        let drc = pdk.drc_alt.as_ref().unwrap();
        // Striped pattern ~50% density, well under 0.77 max
        let mut bitmap = make_striped(16, 16, 4);
        let rects = generate_layer_polygons(
            &mut bitmap,
            &pdk,
            drc,
            PolygonStrategy::RowMerge,
            PixelPlacement::Touching,
            true,
            false,
        )
        .unwrap();
        assert!(
            !rects.is_empty(),
            "Should produce rects with density enforcement enabled"
        );
    }

    #[test]
    fn generate_layer_polygons_force_flag() {
        let pdk = PdkConfig::builtin("sky130").unwrap();
        let drc = pdk.drc_alt.as_ref().unwrap();
        assert!(
            drc.density_max < 1.0,
            "Alt DRC rules should have density_max < 1.0"
        );
        // Use a fully-filled bitmap with Touching placement - maximum density.
        // The density loop may not fully converge, but force=true prevents error.
        let mut bitmap = make_filled(16, 16);
        let result = generate_layer_polygons(
            &mut bitmap,
            &pdk,
            drc,
            PolygonStrategy::RowMerge,
            PixelPlacement::Touching,
            true,
            true,
        );
        assert!(
            result.is_ok(),
            "force=true should not error on density violations"
        );
    }

    #[test]
    fn density_prepass_clears_pixels() {
        let pdk = PdkConfig::builtin("sky130").unwrap();
        // Construct custom DRC rules with a tiny density window so that
        // the enforce_density sliding window fits inside a small test bitmap.
        // sky130 met5 pitch (touching) = max(1.6, 1.6) = 1.6 um.
        // We want window_px = floor(density_window_um / pitch) to be <= bitmap size.
        // With a 32x32 bitmap and pitch 1.6 um, set window to 32 * 1.6 = 51.2 um
        // so window_px = floor(51.2 / 1.6) = 32, which fits exactly.
        let drc = DrcRules {
            min_width: 1.6,
            min_spacing: 1.6,
            min_area: 0.0,
            density_min: 0.0,
            density_max: 0.70,
            density_window_um: 48.0, // floor(48.0 / 1.6) = 30 px window
            max_width: None,
            wide_metal_threshold: None,
            wide_metal_spacing: None,
        };

        let mut bitmap = make_filled(32, 32);
        let density_before = bitmap.density();
        assert!(
            (density_before - 1.0).abs() < f64::EPSILON,
            "Filled bitmap should start at 100% density"
        );

        density_prepass(&mut bitmap, &pdk, &drc, PixelPlacement::Touching);

        let density_after = bitmap.density();
        assert!(
            density_after < density_before,
            "Density prepass should reduce density from {:.3} but got {:.3}",
            density_before,
            density_after
        );
        // The prepass may not reach exactly density_max on small bitmaps
        // (it preserves pixels with few neighbors to maintain thin lines),
        // but it should get close. Allow a small margin.
        assert!(
            density_after < 0.85,
            "Density after prepass ({:.3}) should be significantly reduced toward max ({:.3})",
            density_after,
            drc.density_max
        );
    }
}
