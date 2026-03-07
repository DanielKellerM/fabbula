// Copyright 2026 Daniel Keller <daniel.keller.m@gmail.com>
// Licensed under the Apache License, Version 2.0.
// SPDX-License-Identifier: Apache-2.0

//! CLI regression tests using snapshot testing (insta) and structural assertions.
//!
//! Uses a deterministic 16x16 checkerboard test image generated programmatically.
//! Text outputs (stdout, SVG, LEF) are snapshot-tested; binary GDS is tested via
//! structural stats (polygon count, layers, bounding box).

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::{Path, PathBuf};

/// Build a `Command` for the fabbula binary.
#[allow(deprecated)]
fn fabbula() -> Command {
    Command::cargo_bin("fabbula").expect("fabbula binary not found")
}

/// Generate a deterministic 16x16 checkerboard PNG in the given directory.
/// Uses 4x4 block pattern to ensure contiguous pixels satisfy min_area rules.
fn create_test_image(dir: &Path) -> PathBuf {
    let path = dir.join("checkerboard_16x16.png");
    let mut imgbuf = image::GrayImage::new(16, 16);
    for y in 0..16u32 {
        for x in 0..16u32 {
            // 4x4 block checkerboard: blocks at (0,0), (4,0), (8,0)... alternate
            let block_x = x / 4;
            let block_y = y / 4;
            let on = (block_x + block_y) % 2 == 0;
            imgbuf.put_pixel(x, y, image::Luma([if on { 255 } else { 0 }]));
        }
    }
    imgbuf.save(&path).expect("failed to save test image");
    path
}

/// Read a GDS file and return formatted structural stats for snapshot comparison.
fn gds_stats(path: &Path) -> String {
    let lib =
        gds21::GdsLibrary::load(path).unwrap_or_else(|e| panic!("failed to load GDS: {:?}", e));
    let mut lines = Vec::new();
    for cell in &lib.structs {
        let name = &cell.name;
        let polygon_count = cell.elems.len();

        // Collect unique layers and compute bounding box
        let mut layers = std::collections::BTreeSet::new();
        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;

        for elem in &cell.elems {
            if let gds21::GdsElement::GdsBoundary(b) = elem {
                layers.insert((b.layer, b.datatype));
                for pt in &b.xy {
                    min_x = min_x.min(pt.x);
                    min_y = min_y.min(pt.y);
                    max_x = max_x.max(pt.x);
                    max_y = max_y.max(pt.y);
                }
            }
        }

        lines.push(format!("cell: {}", name));
        lines.push(format!("  polygons: {}", polygon_count));
        let layer_strs: Vec<String> = layers.iter().map(|(l, d)| format!("{}/{}", l, d)).collect();
        lines.push(format!("  layers: [{}]", layer_strs.join(", ")));
        if min_x <= max_x {
            lines.push(format!(
                "  bbox: ({}, {}) to ({}, {})",
                min_x, min_y, max_x, max_y
            ));
        } else {
            lines.push("  bbox: empty".to_string());
        }
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// list-pdks
// ---------------------------------------------------------------------------

#[test]
fn snapshot_list_pdks() {
    let output = fabbula().arg("list-pdks").output().expect("failed to run");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    insta::assert_snapshot!("list_pdks", stdout);
}

// ---------------------------------------------------------------------------
// show-pdk (one per PDK)
// ---------------------------------------------------------------------------

macro_rules! show_pdk_test {
    ($name:ident, $pdk:expr) => {
        #[test]
        fn $name() {
            let output = fabbula()
                .args(["show-pdk", $pdk])
                .output()
                .expect("failed to run");
            assert!(output.status.success());
            let stdout = String::from_utf8_lossy(&output.stdout);
            insta::assert_snapshot!(stdout);
        }
    };
}

show_pdk_test!(snapshot_show_pdk_sky130, "sky130");
show_pdk_test!(snapshot_show_pdk_ihp_sg13g2, "ihp_sg13g2");
show_pdk_test!(snapshot_show_pdk_gf180mcu, "gf180mcu");
show_pdk_test!(snapshot_show_pdk_freepdk45, "freepdk45");
show_pdk_test!(snapshot_show_pdk_asap7, "asap7");
show_pdk_test!(snapshot_show_pdk_fabbula2, "fabbula2");

// ---------------------------------------------------------------------------
// generate - SVG snapshots per PDK
// ---------------------------------------------------------------------------

macro_rules! generate_svg_test {
    ($name:ident, $pdk:expr) => {
        #[test]
        fn $name() {
            let dir = tempfile::tempdir().unwrap();
            let img = create_test_image(dir.path());
            let svg_path = dir.path().join("out.svg");
            let gds_path = dir.path().join("out.gds");

            fabbula()
                .args([
                    "generate",
                    "-i",
                    img.to_str().unwrap(),
                    "-o",
                    gds_path.to_str().unwrap(),
                    "-p",
                    $pdk,
                    "--threshold",
                    "128",
                    "--svg",
                    svg_path.to_str().unwrap(),
                ])
                .assert()
                .success();

            let svg_content = std::fs::read_to_string(&svg_path).expect("SVG file should exist");
            insta::assert_snapshot!(svg_content);
        }
    };
}

generate_svg_test!(snapshot_generate_svg_sky130, "sky130");
generate_svg_test!(snapshot_generate_svg_ihp_sg13g2, "ihp_sg13g2");
generate_svg_test!(snapshot_generate_svg_gf180mcu, "gf180mcu");
generate_svg_test!(snapshot_generate_svg_freepdk45, "freepdk45");
generate_svg_test!(snapshot_generate_svg_asap7, "asap7");
generate_svg_test!(snapshot_generate_svg_fabbula2, "fabbula2");

// ---------------------------------------------------------------------------
// generate - LEF snapshots per PDK
// ---------------------------------------------------------------------------

macro_rules! generate_lef_test {
    ($name:ident, $pdk:expr) => {
        #[test]
        fn $name() {
            let dir = tempfile::tempdir().unwrap();
            let img = create_test_image(dir.path());
            let lef_path = dir.path().join("out.lef");
            let gds_path = dir.path().join("out.gds");

            fabbula()
                .args([
                    "generate",
                    "-i",
                    img.to_str().unwrap(),
                    "-o",
                    gds_path.to_str().unwrap(),
                    "-p",
                    $pdk,
                    "--threshold",
                    "128",
                    "--lef",
                    lef_path.to_str().unwrap(),
                ])
                .assert()
                .success();

            let lef_content = std::fs::read_to_string(&lef_path).expect("LEF file should exist");
            insta::assert_snapshot!(lef_content);
        }
    };
}

generate_lef_test!(snapshot_generate_lef_sky130, "sky130");
generate_lef_test!(snapshot_generate_lef_ihp_sg13g2, "ihp_sg13g2");
generate_lef_test!(snapshot_generate_lef_gf180mcu, "gf180mcu");
generate_lef_test!(snapshot_generate_lef_freepdk45, "freepdk45");
generate_lef_test!(snapshot_generate_lef_asap7, "asap7");
generate_lef_test!(snapshot_generate_lef_fabbula2, "fabbula2");

// ---------------------------------------------------------------------------
// generate - GDS structural stats per PDK
// ---------------------------------------------------------------------------

macro_rules! generate_gds_stats_test {
    ($name:ident, $pdk:expr) => {
        #[test]
        fn $name() {
            let dir = tempfile::tempdir().unwrap();
            let img = create_test_image(dir.path());
            let gds_path = dir.path().join("out.gds");

            fabbula()
                .args([
                    "generate",
                    "-i",
                    img.to_str().unwrap(),
                    "-o",
                    gds_path.to_str().unwrap(),
                    "-p",
                    $pdk,
                    "--threshold",
                    "128",
                ])
                .assert()
                .success();

            let stats = gds_stats(&gds_path);
            insta::assert_snapshot!(stats);
        }
    };
}

generate_gds_stats_test!(snapshot_generate_gds_sky130, "sky130");
generate_gds_stats_test!(snapshot_generate_gds_ihp_sg13g2, "ihp_sg13g2");
generate_gds_stats_test!(snapshot_generate_gds_gf180mcu, "gf180mcu");
generate_gds_stats_test!(snapshot_generate_gds_freepdk45, "freepdk45");
generate_gds_stats_test!(snapshot_generate_gds_asap7, "asap7");
generate_gds_stats_test!(snapshot_generate_gds_fabbula2, "fabbula2");

// ---------------------------------------------------------------------------
// generate - polygon counts per strategy
// ---------------------------------------------------------------------------

#[test]
fn snapshot_generate_strategies() {
    let dir = tempfile::tempdir().unwrap();
    let img = create_test_image(dir.path());
    let mut lines = Vec::new();

    for strategy in [
        "pixel-rects",
        "row-merge",
        "greedy-merge",
        "histogram-merge",
    ] {
        let gds_path = dir.path().join(format!("out_{}.gds", strategy));
        fabbula()
            .args([
                "generate",
                "-i",
                img.to_str().unwrap(),
                "-o",
                gds_path.to_str().unwrap(),
                "-p",
                "sky130",
                "--threshold",
                "128",
                "--strategy",
                strategy,
            ])
            .assert()
            .success();

        let stats = gds_stats(&gds_path);
        lines.push(format!("--- strategy: {} ---", strategy));
        lines.push(stats);
    }

    insta::assert_snapshot!("strategies_polygon_counts", lines.join("\n"));
}

// ---------------------------------------------------------------------------
// DRC check runs by default for all PDKs
// ---------------------------------------------------------------------------

#[test]
fn generate_check_drc_all_pdks() {
    let dir = tempfile::tempdir().unwrap();
    let img = create_test_image(dir.path());

    for pdk in [
        "sky130",
        "ihp_sg13g2",
        "gf180mcu",
        "freepdk45",
        "asap7",
        "fabbula2",
    ] {
        let gds_path = dir.path().join(format!("drc_{}.gds", pdk));
        fabbula()
            .args([
                "generate",
                "-i",
                img.to_str().unwrap(),
                "-o",
                gds_path.to_str().unwrap(),
                "-p",
                pdk,
                "--threshold",
                "128",
            ])
            .assert()
            .success()
            .stderr(predicates::str::contains("VIOLATION").not());
    }
}

// ---------------------------------------------------------------------------
// --separated mode
// ---------------------------------------------------------------------------

#[test]
fn generate_separated_mode() {
    let dir = tempfile::tempdir().unwrap();
    let img = create_test_image(dir.path());
    let gds_path = dir.path().join("separated.gds");

    fabbula()
        .args([
            "generate",
            "-i",
            img.to_str().unwrap(),
            "-o",
            gds_path.to_str().unwrap(),
            "-p",
            "sky130",
            "--threshold",
            "128",
            "--separated",
        ])
        .assert()
        .success();

    let stats = gds_stats(&gds_path);
    insta::assert_snapshot!("separated_mode", stats);
}

// ---------------------------------------------------------------------------
// --invert changes output
// ---------------------------------------------------------------------------

#[test]
fn generate_invert_changes_output() {
    let dir = tempfile::tempdir().unwrap();
    // Use an asymmetric image (mostly white with a few dark pixels) so invert
    // produces a measurably different output.
    let img = dir.path().join("asymmetric.png");
    let mut imgbuf = image::GrayImage::new(16, 16);
    for y in 0..16u32 {
        for x in 0..16u32 {
            // Top-left 8x8 block is dark, rest is white
            let val = if x < 8 && y < 8 { 0u8 } else { 255u8 };
            imgbuf.put_pixel(x, y, image::Luma([val]));
        }
    }
    imgbuf.save(&img).expect("failed to save asymmetric image");
    let gds_normal = dir.path().join("normal.gds");
    let gds_invert = dir.path().join("invert.gds");

    fabbula()
        .args([
            "generate",
            "-i",
            img.to_str().unwrap(),
            "-o",
            gds_normal.to_str().unwrap(),
            "-p",
            "sky130",
            "--threshold",
            "128",
        ])
        .assert()
        .success();

    fabbula()
        .args([
            "generate",
            "-i",
            img.to_str().unwrap(),
            "-o",
            gds_invert.to_str().unwrap(),
            "-p",
            "sky130",
            "--threshold",
            "128",
            "--invert",
        ])
        .assert()
        .success();

    let stats_normal = gds_stats(&gds_normal);
    let stats_invert = gds_stats(&gds_invert);
    assert_ne!(
        stats_normal, stats_invert,
        "inverted output should differ from normal"
    );
}

// ---------------------------------------------------------------------------
// --dither accepted
// ---------------------------------------------------------------------------

#[test]
fn generate_dither_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let img = create_test_image(dir.path());
    let gds_path = dir.path().join("dither.gds");

    fabbula()
        .args([
            "generate",
            "-i",
            img.to_str().unwrap(),
            "-o",
            gds_path.to_str().unwrap(),
            "-p",
            "sky130",
            "--threshold",
            "128",
            "--dither",
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// --threshold otsu accepted
// ---------------------------------------------------------------------------

#[test]
fn generate_threshold_otsu() {
    let dir = tempfile::tempdir().unwrap();
    let img = create_test_image(dir.path());
    let gds_path = dir.path().join("otsu.gds");

    fabbula()
        .args([
            "generate",
            "-i",
            img.to_str().unwrap(),
            "-o",
            gds_path.to_str().unwrap(),
            "-p",
            "sky130",
            "--threshold",
            "otsu",
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[test]
fn error_invalid_pdk() {
    let dir = tempfile::tempdir().unwrap();
    let img = create_test_image(dir.path());
    let gds_path = dir.path().join("out.gds");

    fabbula()
        .args([
            "generate",
            "-i",
            img.to_str().unwrap(),
            "-o",
            gds_path.to_str().unwrap(),
            "-p",
            "nonexistent_pdk",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown built-in PDK"));
}

#[test]
fn error_missing_input_file() {
    let dir = tempfile::tempdir().unwrap();
    let gds_path = dir.path().join("out.gds");

    fabbula()
        .args([
            "generate",
            "-i",
            "/tmp/nonexistent_image_12345.png",
            "-o",
            gds_path.to_str().unwrap(),
            "-p",
            "sky130",
        ])
        .assert()
        .failure();
}

#[test]
fn error_missing_required_args() {
    fabbula()
        .arg("generate")
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ---------------------------------------------------------------------------
// --size-um physical dimension specification
// ---------------------------------------------------------------------------

#[test]
fn generate_size_um() {
    let dir = tempfile::tempdir().unwrap();
    let img = create_test_image(dir.path());
    let gds_path = dir.path().join("size_um.gds");

    fabbula()
        .args([
            "generate",
            "-i",
            img.to_str().unwrap(),
            "-o",
            gds_path.to_str().unwrap(),
            "-p",
            "sky130",
            "--threshold",
            "128",
            "--size-um",
            "100x100",
        ])
        .assert()
        .success();

    let stats = gds_stats(&gds_path);
    insta::assert_snapshot!("size_um", stats);
}

// ---------------------------------------------------------------------------
// --color-mode channel (multi-layer RGB)
// ---------------------------------------------------------------------------

/// Create a deterministic 16x16 RGB test image with distinct red/green/blue regions.
fn create_color_test_image(dir: &Path) -> PathBuf {
    let path = dir.join("color_16x16.png");
    let mut imgbuf = image::RgbImage::new(16, 16);
    for y in 0..16u32 {
        for x in 0..16u32 {
            let pixel = if y < 8 {
                // Top half: red blocks
                if (x / 4 + y / 4) % 2 == 0 {
                    image::Rgb([255, 0, 0])
                } else {
                    image::Rgb([0, 0, 0])
                }
            } else {
                // Bottom half: green blocks
                if (x / 4 + y / 4) % 2 == 0 {
                    image::Rgb([0, 255, 0])
                } else {
                    image::Rgb([0, 0, 0])
                }
            };
            imgbuf.put_pixel(x, y, pixel);
        }
    }
    imgbuf.save(&path).expect("failed to save color test image");
    path
}

/// Create a custom PDK TOML with color fields for channel mode testing.
fn create_channel_pdk(dir: &Path) -> PathBuf {
    let path = dir.join("channel_pdk.toml");
    std::fs::write(
        &path,
        r#"
[pdk]
name = "channel_test"
description = "Test PDK with color fields"
node_nm = 130
db_units_per_um = 1000

[artwork_layer]
name = "red_layer"
gds_layer = 72
gds_datatype = 20
purpose = "drawing"

[drc]
min_width = 1.600
min_spacing = 1.600
min_area = 4.000
density_min = 0.0
density_max = 1.0
density_window_um = 700.0

[grid]
manufacturing_grid_um = 0.005

[[artwork_layers]]
name = "red_layer"
gds_layer = 72
gds_datatype = 20
color = "red"
[artwork_layers.drc]
min_width = 1.600
min_spacing = 1.600
min_area = 4.000
density_min = 0.0
density_max = 1.0
density_window_um = 700.0

[[artwork_layers]]
name = "green_layer"
gds_layer = 71
gds_datatype = 20
color = "green"
[artwork_layers.drc]
min_width = 1.600
min_spacing = 1.600
min_area = 4.000
density_min = 0.0
density_max = 1.0
density_window_um = 700.0
"#,
    )
    .expect("failed to write channel PDK");
    path
}

#[test]
fn generate_color_mode_channel() {
    let dir = tempfile::tempdir().unwrap();
    let img = create_color_test_image(dir.path());
    let pdk_path = create_channel_pdk(dir.path());
    let gds_path = dir.path().join("channel.gds");

    fabbula()
        .args([
            "generate",
            "-i",
            img.to_str().unwrap(),
            "-o",
            gds_path.to_str().unwrap(),
            "-p",
            pdk_path.to_str().unwrap(),
            "--threshold",
            "128",
            "--color-mode",
            "channel",
        ])
        .assert()
        .success();

    let stats = gds_stats(&gds_path);
    // Channel mode should produce polygons on multiple GDS layers
    assert!(
        stats.contains("72/20"),
        "should have red artwork layer: {}",
        stats
    );
    insta::assert_snapshot!("color_mode_channel", stats);
}

// ---------------------------------------------------------------------------
// --color-mode palette (k-means multi-layer)
// ---------------------------------------------------------------------------

#[test]
fn generate_color_mode_palette() {
    let dir = tempfile::tempdir().unwrap();
    let img = create_color_test_image(dir.path());
    let gds_path = dir.path().join("palette.gds");

    fabbula()
        .args([
            "generate",
            "-i",
            img.to_str().unwrap(),
            "-o",
            gds_path.to_str().unwrap(),
            "-p",
            "sky130",
            "--threshold",
            "128",
            "--color-mode",
            "palette",
            "--num-colors",
            "2",
        ])
        .assert()
        .success();

    let stats = gds_stats(&gds_path);
    insta::assert_snapshot!("color_mode_palette", stats);
}

// ---------------------------------------------------------------------------
// --html preview output
// ---------------------------------------------------------------------------

#[test]
fn generate_html_preview() {
    let dir = tempfile::tempdir().unwrap();
    let img = create_test_image(dir.path());
    let gds_path = dir.path().join("out.gds");
    let html_path = dir.path().join("preview.html");

    fabbula()
        .args([
            "generate",
            "-i",
            img.to_str().unwrap(),
            "-o",
            gds_path.to_str().unwrap(),
            "-p",
            "sky130",
            "--threshold",
            "128",
            "--html",
            html_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    let html_content = std::fs::read_to_string(&html_path).expect("HTML file should exist");
    assert!(html_content.contains("<html"), "should be valid HTML");
    assert!(html_content.contains("svg"), "should contain SVG elements");
    // HTML has embedded JS with dynamic content; just verify key structural elements
    assert!(
        html_content.len() > 500,
        "HTML preview should have substantial content"
    );
}

// ---------------------------------------------------------------------------
// merge subcommand
// ---------------------------------------------------------------------------

/// Create a minimal GDS file to use as merge target.
fn create_chip_gds(dir: &Path) -> PathBuf {
    let path = dir.join("chip.gds");
    let mut lib = gds21::GdsLibrary::new("testlib");
    let mut cell = gds21::GdsStruct::new("top_cell");
    // Add a single boundary on a different layer to distinguish from artwork
    let boundary = gds21::GdsBoundary {
        layer: 1,
        datatype: 0,
        xy: vec![
            gds21::GdsPoint::new(0, 0),
            gds21::GdsPoint::new(100000, 0),
            gds21::GdsPoint::new(100000, 100000),
            gds21::GdsPoint::new(0, 100000),
            gds21::GdsPoint::new(0, 0),
        ],
        ..Default::default()
    };
    cell.elems.push(gds21::GdsElement::GdsBoundary(boundary));
    lib.structs.push(cell);
    lib.save(&path).expect("failed to save chip GDS");
    path
}

#[test]
fn merge_into_existing_gds() {
    let dir = tempfile::tempdir().unwrap();
    let img = create_test_image(dir.path());
    let chip = create_chip_gds(dir.path());
    let out_path = dir.path().join("merged.gds");

    fabbula()
        .args([
            "merge",
            "-i",
            img.to_str().unwrap(),
            "--chip",
            chip.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
            "-p",
            "sky130",
            "--threshold",
            "128",
        ])
        .assert()
        .success();

    let stats = gds_stats(&out_path);
    // Merged GDS should have both the original chip boundary (layer 1/0)
    // and artwork polygons (layer 72/20)
    assert!(
        stats.contains("1/0"),
        "should preserve chip layer: {}",
        stats
    );
    assert!(
        stats.contains("72/20"),
        "should have artwork layer: {}",
        stats
    );
    insta::assert_snapshot!("merge_into_gds", stats);
}

#[test]
fn merge_with_offset() {
    let dir = tempfile::tempdir().unwrap();
    let img = create_test_image(dir.path());
    let chip = create_chip_gds(dir.path());
    let out_path = dir.path().join("merged_offset.gds");

    fabbula()
        .args([
            "merge",
            "-i",
            img.to_str().unwrap(),
            "--chip",
            chip.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
            "-p",
            "sky130",
            "--threshold",
            "128",
            "--offset-x",
            "10.0",
            "--offset-y",
            "20.0",
        ])
        .assert()
        .success();

    let stats = gds_stats(&out_path);
    insta::assert_snapshot!("merge_with_offset", stats);
}

#[test]
fn merge_separated_mode() {
    let dir = tempfile::tempdir().unwrap();
    let img = create_test_image(dir.path());
    let chip = create_chip_gds(dir.path());
    let out_path = dir.path().join("merged_sep.gds");

    fabbula()
        .args([
            "merge",
            "-i",
            img.to_str().unwrap(),
            "--chip",
            chip.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
            "-p",
            "sky130",
            "--threshold",
            "128",
            "--separated",
        ])
        .assert()
        .success();

    assert!(out_path.exists());
}

// ---------------------------------------------------------------------------
// SVG snapshot platform stability check
// ---------------------------------------------------------------------------

#[test]
fn svg_output_is_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    let img = create_test_image(dir.path());
    let svg1 = dir.path().join("run1.svg");
    let svg2 = dir.path().join("run2.svg");
    let gds1 = dir.path().join("run1.gds");
    let gds2 = dir.path().join("run2.gds");

    for (svg, gds) in [(&svg1, &gds1), (&svg2, &gds2)] {
        fabbula()
            .args([
                "generate",
                "-i",
                img.to_str().unwrap(),
                "-o",
                gds.to_str().unwrap(),
                "-p",
                "sky130",
                "--threshold",
                "128",
                "--svg",
                svg.to_str().unwrap(),
            ])
            .assert()
            .success();
    }

    let content1 = std::fs::read_to_string(&svg1).unwrap();
    let content2 = std::fs::read_to_string(&svg2).unwrap();
    assert_eq!(
        content1, content2,
        "SVG output should be deterministic across runs"
    );
}
