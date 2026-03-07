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
                let tight_max = drc_rules.density_max * 0.95;
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

    let final_violations =
        check_density_only(&best_rects, pdk.pdk.db_units_per_um, drc_rules, Some(1));
    if !final_violations.is_empty() {
        tracing::warn!(
            "Density loop exhausted {} retries with violations remaining",
            max_retries
        );
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
    if window_px > 0 {
        let cleared = enforce_density(bitmap, drc.density_max, window_px);
        if cleared > 0 {
            tracing::info!("Density enforcement: cleared {} pixels", cleared);
        }
    }
}

/// Generate polygons for a single layer with optional density enforcement.
pub fn generate_layer_polygons(
    bitmap: &mut ArtworkBitmap,
    pdk: &PdkConfig,
    drc: &DrcRules,
    strategy: PolygonStrategy,
    placement: PixelPlacement,
    density_enforce: bool,
) -> Result<Vec<Rect>> {
    if density_enforce && drc.density_max < 1.0 {
        density_prepass(bitmap, pdk, drc, placement);
        generate_with_density_loop(bitmap, pdk, drc, strategy, placement, 3)
    } else {
        generate_polygons(bitmap, pdk, drc, strategy, placement)
    }
}
