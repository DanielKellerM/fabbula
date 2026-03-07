// Copyright 2026 Daniel Keller <daniel.keller.m@gmail.com>
// Licensed under the Apache License, Version 2.0.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end integration tests exercising the full pipeline:
//! image -> bitmap -> polygons -> DRC check -> GDS output.

use fabbula::artwork::{ArtworkBitmap, DitherMode, ThresholdMode, load_artwork};
use fabbula::drc::{DrcRule, check_drc};
use fabbula::gdsio::write_gds;
use fabbula::pdk::PdkConfig;
use fabbula::polygon::{PixelPlacement, PolygonStrategy, generate_polygons};

/// Full pipeline: load_artwork -> generate_polygons -> DRC -> write_gds.
/// Uses a programmatic 32x32 checkerboard PNG (no external files needed).
#[test]
fn generate_sky130_from_image() {
    let dir = tempfile::tempdir().unwrap();
    let image_path = dir.path().join("checkerboard.png");
    let mut imgbuf = image::GrayImage::new(32, 32);
    for y in 0..32u32 {
        for x in 0..32u32 {
            let on = (x / 4 + y / 4) % 2 == 0;
            imgbuf.put_pixel(x, y, image::Luma([if on { 255 } else { 0 }]));
        }
    }
    imgbuf.save(&image_path).unwrap();

    let pdk = PdkConfig::builtin("sky130").unwrap();
    let bitmap = load_artwork(
        &image_path,
        ThresholdMode::Luminance(128),
        Some((32, 32)),
        DitherMode::Off,
    )
    .unwrap();
    assert!(bitmap.width > 0 && bitmap.height > 0);

    let rects = generate_polygons(
        &bitmap,
        &pdk,
        &pdk.drc,
        PolygonStrategy::GreedyMerge,
        PixelPlacement::Touching,
    )
    .unwrap();
    assert!(!rects.is_empty(), "should produce polygons from image");

    let violations = check_drc(&rects, pdk.pdk.db_units_per_um, &pdk.drc);
    assert!(violations.is_empty(), "DRC violations: {:?}", violations);

    let gds_path = dir.path().join("checkerboard_sky130.gds");
    write_gds(&rects, &pdk, "checkerboard_art", &gds_path).unwrap();
    assert!(gds_path.exists());
    assert!(
        std::fs::metadata(&gds_path).unwrap().len() > 0,
        "GDS file should not be empty"
    );
}

/// Full pipeline with separated mode across all PDKs.
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

    for builtin in PdkConfig::list_builtins() {
        let pdk = PdkConfig::builtin(builtin.name()).unwrap();
        let rects = generate_polygons(
            &bitmap,
            &pdk,
            &pdk.drc,
            PolygonStrategy::RowMerge,
            PixelPlacement::Separated,
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
            builtin.name(),
            spacing_violations
        );
    }
}

/// Empty bitmap (all white / no metal) produces zero polygons and valid GDS.
#[test]
fn empty_bitmap_produces_empty_gds() {
    let bitmap = ArtworkBitmap::from_bools(16, 16, &[false; 256]);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    let rects = generate_polygons(
        &bitmap,
        &pdk,
        &pdk.drc,
        PolygonStrategy::GreedyMerge,
        PixelPlacement::Separated,
    )
    .unwrap();
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
    let rects = generate_polygons(
        &bitmap,
        &pdk,
        &pdk.drc,
        PolygonStrategy::PixelRects,
        PixelPlacement::Separated,
    )
    .unwrap();
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

    for builtin in PdkConfig::list_builtins() {
        let pdk = PdkConfig::builtin(builtin.name()).unwrap();
        for strategy in [
            PolygonStrategy::PixelRects,
            PolygonStrategy::RowMerge,
            PolygonStrategy::GreedyMerge,
            PolygonStrategy::HistogramMerge,
        ] {
            let rects =
                generate_polygons(&bitmap, &pdk, &pdk.drc, strategy, PixelPlacement::Touching)
                    .unwrap();
            let violations = check_drc(&rects, pdk.pdk.db_units_per_um, &pdk.drc);
            assert!(
                violations.is_empty(),
                "{} {:?} touching mode DRC violations: {:?}",
                builtin.name(),
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
        let rects = generate_polygons(&bitmap, &pdk, &pdk.drc, strategy, PixelPlacement::Separated)
            .unwrap();
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
#[test]
fn coordinate_overflow_detected() {
    let bitmap = ArtworkBitmap::from_bools(700_000, 1, &vec![false; 700_000]);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    let result = generate_polygons(
        &bitmap,
        &pdk,
        &pdk.drc,
        PolygonStrategy::PixelRects,
        PixelPlacement::Separated,
    );
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
    let rects = generate_polygons(
        &bitmap,
        &pdk,
        &pdk.drc,
        PolygonStrategy::PixelRects,
        PixelPlacement::Separated,
    )
    .unwrap();
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
        PixelPlacement::Separated,
    )
    .unwrap();
    assert!(
        !rects2.is_empty(),
        "2x2 block should pass min_area on sky130"
    );
}
