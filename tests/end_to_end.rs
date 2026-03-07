// Copyright 2026 Daniel Keller <daniel.keller.m@gmail.com>
// Licensed under the Apache License, Version 2.0.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end integration tests exercising the full pipeline:
//! image -> bitmap -> polygons -> DRC check -> GDS output.

use fabbula::artwork::{ArtworkBitmap, ThresholdMode, load_artwork};
use fabbula::drc::{DrcRule, check_drc};
use fabbula::gdsio::write_gds;
use fabbula::pdk::PdkConfig;
use fabbula::polygon::{PolygonStrategy, generate_polygons};
use std::path::Path;

/// Full pipeline with a real image and sky130 PDK.
#[test]
fn generate_sky130_from_image() {
    let pdk = PdkConfig::builtin("sky130").unwrap();
    let bitmap = load_artwork(
        Path::new("media/input/bear.png"),
        ThresholdMode::Otsu,
        Some((128, 128)),
        false,
    )
    .unwrap();
    assert!(bitmap.width > 0 && bitmap.height > 0);

    let rects =
        generate_polygons(&bitmap, &pdk, &pdk.drc, PolygonStrategy::GreedyMerge, false).unwrap();
    assert!(!rects.is_empty(), "should produce polygons from image");

    let violations = check_drc(&rects, pdk.pdk.db_units_per_um, &pdk.drc);
    assert!(violations.is_empty(), "DRC violations: {:?}", violations);

    let dir = tempfile::tempdir().unwrap();
    let gds_path = dir.path().join("bear_sky130.gds");
    write_gds(&rects, &pdk, "bear_art", &gds_path).unwrap();
    assert!(gds_path.exists());
    assert!(
        std::fs::metadata(&gds_path).unwrap().len() > 0,
        "GDS file should not be empty"
    );
}

/// Full pipeline with touching mode across all PDKs.
#[test]
fn generate_all_pdks_separated_mode() {
    let bitmap = ArtworkBitmap::from_bools(
        8,
        8,
        &[
            false, true, true, false, false, true, true, false, true, false, true, false, true,
            false, true, false, false, true, true, false, false, true, true, false, true, false,
            false, true, true, false, false, true, false, true, true, false, false, true, true,
            false, true, false, true, false, true, false, true, false, false, true, true, false,
            false, true, true, false, true, false, false, true, true, false, false, true,
        ],
    );

    for name in PdkConfig::list_builtins() {
        let pdk = PdkConfig::builtin(name).unwrap();
        let rects = generate_polygons(
            &bitmap,
            &pdk,
            &pdk.drc,
            PolygonStrategy::RowMerge,
            false, // separated mode - guaranteed DRC-clean
        )
        .unwrap();

        let violations = check_drc(&rects, pdk.pdk.db_units_per_um, &pdk.drc);
        let spacing_violations: Vec<_> = violations
            .iter()
            .filter(|v| matches!(v.rule, DrcRule::MinSpacing))
            .collect();
        assert!(
            spacing_violations.is_empty(),
            "{}: spacing violations in separated mode: {:?}",
            name,
            spacing_violations
        );
    }
}

/// Empty bitmap (all white / no metal) produces zero polygons and valid GDS.
#[test]
fn empty_bitmap_produces_empty_gds() {
    let bitmap = ArtworkBitmap::from_bools(16, 16, &[false; 256]);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    let rects =
        generate_polygons(&bitmap, &pdk, &pdk.drc, PolygonStrategy::GreedyMerge, false).unwrap();
    assert!(rects.is_empty(), "all-white bitmap should produce 0 rects");

    let dir = tempfile::tempdir().unwrap();
    let gds_path = dir.path().join("empty.gds");
    write_gds(&rects, &pdk, "empty", &gds_path).unwrap();
    assert!(gds_path.exists());
}

/// Single pixel produces exactly one rectangle (using PDK with min_area=0).
#[test]
fn single_pixel_bitmap() {
    let bitmap = ArtworkBitmap::from_bools(1, 1, &[true]);
    // IHP has min_area=0, so single pixel won't be filtered
    let pdk = PdkConfig::builtin("ihp_sg13g2").unwrap();
    let rects =
        generate_polygons(&bitmap, &pdk, &pdk.drc, PolygonStrategy::PixelRects, false).unwrap();
    assert_eq!(rects.len(), 1, "single pixel should produce exactly 1 rect");

    let violations = check_drc(&rects, pdk.pdk.db_units_per_um, &pdk.drc);
    let width_violations: Vec<_> = violations
        .iter()
        .filter(|v| matches!(v.rule, DrcRule::MinWidth))
        .collect();
    assert!(
        width_violations.is_empty(),
        "single pixel should satisfy min_width"
    );
}

/// Touching mode produces DRC-clean output for PDKs where eff_spacing > min_width.
/// Regression test: GF180MCU and FreePDK45 had sub-spacing gaps in PixelRects
/// when pixel_w was set to min_width instead of pitch.
#[test]
fn touching_mode_all_pdks_all_strategies() {
    let bitmap = ArtworkBitmap::from_bools(
        6,
        6,
        &[
            true, true, false, true, false, true, true, false, true, true, true, false, false,
            true, true, false, true, true, true, true, false, true, false, false, false, true,
            true, true, true, false, true, false, true, false, true, true,
        ],
    );

    for name in PdkConfig::list_builtins() {
        let pdk = PdkConfig::builtin(name).unwrap();
        for strategy in [
            PolygonStrategy::PixelRects,
            PolygonStrategy::RowMerge,
            PolygonStrategy::GreedyMerge,
            PolygonStrategy::HistogramMerge,
        ] {
            let rects = generate_polygons(&bitmap, &pdk, &pdk.drc, strategy, true).unwrap();
            let violations = check_drc(&rects, pdk.pdk.db_units_per_um, &pdk.drc);
            assert!(
                violations.is_empty(),
                "{} {:?} touching mode DRC violations: {:?}",
                name,
                strategy,
                violations
            );
        }
    }
}

/// All strategies produce DRC-clean output for the same input.
#[test]
fn all_strategies_drc_clean() {
    let bitmap = ArtworkBitmap::from_bools(
        8,
        8,
        &[
            true, true, false, false, true, true, false, false, true, true, false, false, true,
            true, false, false, false, false, true, true, false, false, true, true, false, false,
            true, true, false, false, true, true, true, true, false, false, true, true, false,
            false, true, true, false, false, true, true, false, false, false, false, true, true,
            false, false, true, true, false, false, true, true, false, false, true, true,
        ],
    );

    let pdk = PdkConfig::builtin("sky130").unwrap();
    for strategy in [
        PolygonStrategy::PixelRects,
        PolygonStrategy::RowMerge,
        PolygonStrategy::GreedyMerge,
        PolygonStrategy::HistogramMerge,
    ] {
        let rects = generate_polygons(&bitmap, &pdk, &pdk.drc, strategy, false).unwrap();
        let violations = check_drc(&rects, pdk.pdk.db_units_per_um, &pdk.drc);
        assert!(
            violations.is_empty(),
            "{:?} produced DRC violations: {:?}",
            strategy,
            violations
        );
    }
}

/// Coordinate overflow is detected for extremely large bitmaps.
/// sky130 pitch = 3200 dbu. Need dim > i32::MAX / 3200 ~ 671,000 to overflow.
/// We test with a unit test in polygon.rs instead since creating a 671K bitmap
/// is impractical. Here we verify the overflow guard exists by checking the
/// error message format with a custom PDK-like setup.
#[test]
fn coordinate_overflow_detected() {
    // Use a bitmap just large enough to overflow: 700,000 * 3200 > i32::MAX
    // But 700K*700K bools = 490 billion, that's way too much memory.
    // Instead, use a reasonable size but verify the check exists via a narrow bitmap.
    // With sky130 pitch=3200, width=700000 -> max_x = 699999*3200+1600 = 2,239,998,400 > i32::MAX
    // ArtworkBitmap stores Vec<bool>, so 700000*1 = 700K bools = ~700KB, OK.
    let bitmap = ArtworkBitmap::from_bools(700_000, 1, &vec![false; 700_000]);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    let result = generate_polygons(&bitmap, &pdk, &pdk.drc, PolygonStrategy::PixelRects, false);
    assert!(result.is_err(), "should detect coordinate overflow");
    assert!(
        result.unwrap_err().to_string().contains("overflow"),
        "error should mention overflow"
    );
}

/// min_area filtering removes small rects with sky130 (min_area = 4.0 um^2).
#[test]
fn min_area_filters_small_rects() {
    // Single pixel with sky130: pixel_w = 1.6um -> area = 2.56 um^2 < min_area 4.0
    let bitmap = ArtworkBitmap::from_bools(1, 1, &[true]);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    let rects =
        generate_polygons(&bitmap, &pdk, &pdk.drc, PolygonStrategy::PixelRects, false).unwrap();
    assert!(
        rects.is_empty(),
        "single pixel should be filtered by min_area on sky130"
    );

    // 2x2 block with sky130: merged rect = 2*1.6+0.8 = 4.0um wide -> area >= min_area
    let bitmap2 = ArtworkBitmap::from_bools(2, 2, &[true, true, true, true]);
    let rects2 = generate_polygons(
        &bitmap2,
        &pdk,
        &pdk.drc,
        PolygonStrategy::GreedyMerge,
        false,
    )
    .unwrap();
    assert!(
        !rects2.is_empty(),
        "2x2 block should pass min_area on sky130"
    );
}
