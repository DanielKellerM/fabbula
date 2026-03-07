// Copyright 2026 Daniel Keller <daniel.keller.m@gmail.com>
// Licensed under the Apache License, Version 2.0.
// SPDX-License-Identifier: Apache-2.0

//! GDSII file reading, writing, and merging.
//!
//! Provides functions to write artwork polygons to new GDS files, merge them
//! into existing layouts, and read back existing metal for exclusion masking.

use crate::pdk::PdkConfig;
use crate::polygon::Rect;
use anyhow::Result;
use gds21::{GdsBoundary, GdsLibrary, GdsPoint, GdsStrans, GdsStruct};
use std::collections::HashMap;
use std::io::Read as _;
use std::path::Path;

/// Load a GDS library, transparently decompressing .gz files.
fn load_gds(path: &Path) -> Result<GdsLibrary> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext.eq_ignore_ascii_case("gz") {
        let file = std::fs::File::open(path)
            .map_err(|e| anyhow::anyhow!("Failed to open {}: {}", path.display(), e))?;
        let mut decoder = flate2::read::GzDecoder::new(file);
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .map_err(|e| anyhow::anyhow!("Failed to decompress {}: {}", path.display(), e))?;
        let tmp_dir = std::env::temp_dir();
        let tmp_path = tmp_dir.join(format!("fabbula_decompress_{}.gds", std::process::id()));
        std::fs::write(&tmp_path, &decompressed)?;
        tracing::info!(
            "Decompressed {} ({:.1} MB)",
            path.display(),
            decompressed.len() as f64 / 1_000_000.0
        );
        let lib = GdsLibrary::load(&tmp_path).map_err(|e| {
            anyhow::anyhow!(
                "Failed to read decompressed GDSII {}: {:?}",
                path.display(),
                e
            )
        });
        let _ = std::fs::remove_file(&tmp_path);
        lib
    } else {
        GdsLibrary::load(path)
            .map_err(|e| anyhow::anyhow!("Failed to read GDSII {}: {:?}", path.display(), e))
    }
}

fn format_cell_list(structs: &[GdsStruct]) -> String {
    if structs.len() <= 20 {
        structs
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        let first20: String = structs[..20]
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        format!("{} ... and {} more", first20, structs.len() - 20)
    }
}

fn make_boundary(
    rect: &Rect,
    layer: i16,
    datatype: i16,
    offset_x: i32,
    offset_y: i32,
) -> GdsBoundary {
    GdsBoundary {
        layer,
        datatype,
        xy: vec![
            GdsPoint::new(rect.x0 + offset_x, rect.y0 + offset_y),
            GdsPoint::new(rect.x1 + offset_x, rect.y0 + offset_y),
            GdsPoint::new(rect.x1 + offset_x, rect.y1 + offset_y),
            GdsPoint::new(rect.x0 + offset_x, rect.y1 + offset_y),
            GdsPoint::new(rect.x0 + offset_x, rect.y0 + offset_y),
        ],
        ..Default::default()
    }
}

/// Rectangles associated with a specific GDS layer/datatype pair.
pub struct LayerRects<'a> {
    pub rects: &'a [Rect],
    pub layer: i16,
    pub datatype: i16,
}

/// Write multiple layers of polygons to a new GDSII file.
pub fn write_gds_multi(layers: &[LayerRects], cell_name: &str, output: &Path) -> Result<()> {
    let mut lib = GdsLibrary::new("fabbula");
    let mut cell = GdsStruct::new(cell_name);

    let total: usize = layers.iter().map(|lr| lr.rects.len()).sum();
    cell.elems.reserve(total);
    for lr in layers {
        for rect in lr.rects {
            let boundary = make_boundary(rect, lr.layer, lr.datatype, 0, 0);
            cell.elems.push(gds21::GdsElement::GdsBoundary(boundary));
        }
    }

    lib.structs.push(cell);

    lib.save(output)
        .map_err(|e| anyhow::anyhow!("Failed to write GDSII {}: {:?}", output.display(), e))?;

    tracing::info!(
        "Wrote {} polygons ({} layers) to {} (cell: {})",
        total,
        layers.len(),
        output.display(),
        cell_name,
    );

    Ok(())
}

/// Write polygons to a new GDSII file (single layer).
pub fn write_gds(rects: &[Rect], pdk: &PdkConfig, cell_name: &str, output: &Path) -> Result<()> {
    write_gds_multi(
        &[LayerRects {
            rects,
            layer: pdk.artwork_layer.gds_layer,
            datatype: pdk.artwork_layer.gds_datatype,
        }],
        cell_name,
        output,
    )
}

/// Merge multiple layers of artwork polygons into an existing GDSII file.
pub fn merge_into_gds_multi(
    layers: &[LayerRects],
    input_gds: &Path,
    output_gds: &Path,
    target_cell: Option<&str>,
    offset_x: i32,
    offset_y: i32,
) -> Result<()> {
    let mut lib = load_gds(input_gds)?;

    let cell = if let Some(name) = target_cell {
        let available = format_cell_list(&lib.structs);
        lib.structs
            .iter_mut()
            .find(|s| s.name == name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Cell '{}' not found in GDS. Available cells: {}",
                    name,
                    available
                )
            })?
    } else {
        lib.structs
            .last_mut()
            .ok_or_else(|| anyhow::anyhow!("No cells in input GDS"))?
    };

    let total: usize = layers.iter().map(|lr| lr.rects.len()).sum();
    cell.elems.reserve(total);
    for lr in layers {
        for rect in lr.rects {
            let boundary = make_boundary(rect, lr.layer, lr.datatype, offset_x, offset_y);
            cell.elems.push(gds21::GdsElement::GdsBoundary(boundary));
        }
    }

    lib.save(output_gds)
        .map_err(|e| anyhow::anyhow!("Failed to write GDSII {}: {:?}", output_gds.display(), e))?;

    tracing::info!(
        "Merged {} artwork polygons ({} layers) into {} -> {}",
        total,
        layers.len(),
        input_gds.display(),
        output_gds.display()
    );

    Ok(())
}

/// Merge artwork polygons into an existing GDSII file (single layer).
pub fn merge_into_gds(
    rects: &[Rect],
    pdk: &PdkConfig,
    input_gds: &Path,
    output_gds: &Path,
    target_cell: Option<&str>,
    offset_x: i32,
    offset_y: i32,
) -> Result<()> {
    merge_into_gds_multi(
        &[LayerRects {
            rects,
            layer: pdk.artwork_layer.gds_layer,
            datatype: pdk.artwork_layer.gds_datatype,
        }],
        input_gds,
        output_gds,
        target_cell,
        offset_x,
        offset_y,
    )
}

/// Accumulated GDS transformation (reflect, rotate, magnify, translate).
/// GDS spec order: reflect about x-axis, then rotate, then magnify, then translate.
#[derive(Clone, Debug)]
struct Transform {
    offset_x: f64,
    offset_y: f64,
    angle_deg: f64,
    reflected: bool,
    mag: f64,
}

impl Transform {
    fn identity() -> Self {
        Self {
            offset_x: 0.0,
            offset_y: 0.0,
            angle_deg: 0.0,
            reflected: false,
            mag: 1.0,
        }
    }

    /// Apply this transform to a GDS point, returning integer coordinates.
    fn apply(&self, p: &GdsPoint) -> (i32, i32) {
        let mut x = p.x as f64;
        let mut y = p.y as f64;

        // Step 1: reflect about x-axis
        if self.reflected {
            y = -y;
        }

        // Step 2: rotate (optimize common angles for exact integer math)
        let angle = self.angle_deg.rem_euclid(360.0);
        let (rx, ry) = if (angle - 0.0).abs() < 1e-6 {
            (x, y)
        } else if (angle - 90.0).abs() < 1e-6 {
            (-y, x)
        } else if (angle - 180.0).abs() < 1e-6 {
            (-x, -y)
        } else if (angle - 270.0).abs() < 1e-6 {
            (y, -x)
        } else {
            let rad = angle.to_radians();
            let (sin, cos) = rad.sin_cos();
            (x * cos - y * sin, x * sin + y * cos)
        };
        x = rx;
        y = ry;

        // Step 3: magnify
        x *= self.mag;
        y *= self.mag;

        // Step 4: translate
        x += self.offset_x;
        y += self.offset_y;

        (x.round() as i32, y.round() as i32)
    }

    /// Create a child transform by composing parent transform with SREF/AREF placement.
    fn compose(&self, strans: Option<&GdsStrans>, ref_x: i32, ref_y: i32) -> Transform {
        let child_reflected = strans.is_some_and(|s| s.reflected);
        let child_angle = strans.and_then(|s| s.angle).unwrap_or(0.0);
        let child_mag = strans.and_then(|s| s.mag).unwrap_or(1.0);

        // The child transform is applied first (to geometry in the child cell),
        // then the parent transform is applied. We compose them:
        // parent(child(p)) = parent_translate + parent_mag * parent_rotate * parent_reflect *
        //                     (child_translate + child_mag * child_rotate * child_reflect * p)
        //
        // For simplicity, compute the child's origin in parent coords, then combine angles/reflects.

        // Child's origin offset (ref_x, ref_y) needs to go through parent transform
        let child_origin = GdsPoint::new(ref_x, ref_y);
        let (ox, oy) = self.apply(&child_origin);

        // Combine reflections: reflect XOR
        let combined_reflected = self.reflected ^ child_reflected;

        // Combine angles: if parent is reflected, child angle direction flips
        let effective_child_angle = if self.reflected {
            -child_angle
        } else {
            child_angle
        };
        let combined_angle = self.angle_deg + effective_child_angle;

        let combined_mag = self.mag * child_mag;

        Transform {
            offset_x: ox as f64,
            offset_y: oy as f64,
            angle_deg: combined_angle,
            reflected: combined_reflected,
            mag: combined_mag,
        }
    }
}

const MAX_HIERARCHY_DEPTH: usize = 64;

/// Recursively flatten a GDS cell hierarchy, collecting bounding-box rects
/// for all geometry on the target layer/datatype.
fn flatten_cell(
    cell: &GdsStruct,
    cell_map: &HashMap<&str, &GdsStruct>,
    transform: &Transform,
    layer: i16,
    datatype: i16,
    rects: &mut Vec<Rect>,
    depth: usize,
) {
    if depth >= MAX_HIERARCHY_DEPTH {
        tracing::warn!(
            "Hierarchy depth limit ({}) reached in cell '{}', skipping",
            MAX_HIERARCHY_DEPTH,
            cell.name
        );
        return;
    }

    for elem in &cell.elems {
        match elem {
            gds21::GdsElement::GdsBoundary(b) if b.layer == layer && b.datatype == datatype => {
                if b.xy.is_empty() {
                    continue;
                }
                // Compute bounding box of all transformed points
                let mut x0 = i32::MAX;
                let mut y0 = i32::MAX;
                let mut x1 = i32::MIN;
                let mut y1 = i32::MIN;
                for p in &b.xy {
                    let (tx, ty) = transform.apply(p);
                    x0 = x0.min(tx);
                    y0 = y0.min(ty);
                    x1 = x1.max(tx);
                    y1 = y1.max(ty);
                }
                if x0 < x1 && y0 < y1 {
                    rects.push(Rect::new(x0, y0, x1, y1));
                }
            }
            gds21::GdsElement::GdsPath(p) if p.layer == layer && p.datatype == datatype => {
                if p.xy.is_empty() {
                    continue;
                }
                let half_w = p.width.unwrap_or(0).max(0) / 2;
                let mut x0 = i32::MAX;
                let mut y0 = i32::MAX;
                let mut x1 = i32::MIN;
                let mut y1 = i32::MIN;
                for pt in &p.xy {
                    let (tx, ty) = transform.apply(pt);
                    x0 = x0.min(tx - half_w);
                    y0 = y0.min(ty - half_w);
                    x1 = x1.max(tx + half_w);
                    y1 = y1.max(ty + half_w);
                }
                if x0 < x1 && y0 < y1 {
                    rects.push(Rect::new(x0, y0, x1, y1));
                }
            }
            gds21::GdsElement::GdsStructRef(sref) => {
                if let Some(child) = cell_map.get(sref.name.as_str()) {
                    let child_transform =
                        transform.compose(sref.strans.as_ref(), sref.xy.x, sref.xy.y);
                    flatten_cell(
                        child,
                        cell_map,
                        &child_transform,
                        layer,
                        datatype,
                        rects,
                        depth + 1,
                    );
                } else {
                    tracing::debug!("SREF to unknown cell '{}', skipping", sref.name);
                }
            }
            gds21::GdsElement::GdsArrayRef(aref) => {
                if let Some(child) = cell_map.get(aref.name.as_str()) {
                    // AREF xy: [0] = origin, [1] = origin + cols*col_pitch, [2] = origin + rows*row_pitch
                    let origin = &aref.xy[0];
                    let cols = aref.cols.max(0) as i32;
                    let rows = aref.rows.max(0) as i32;

                    let col_pitch_x = if cols > 0 {
                        (aref.xy[1].x - origin.x) / cols
                    } else {
                        0
                    };
                    let col_pitch_y = if cols > 0 {
                        (aref.xy[1].y - origin.y) / cols
                    } else {
                        0
                    };
                    let row_pitch_x = if rows > 0 {
                        (aref.xy[2].x - origin.x) / rows
                    } else {
                        0
                    };
                    let row_pitch_y = if rows > 0 {
                        (aref.xy[2].y - origin.y) / rows
                    } else {
                        0
                    };

                    for r in 0..rows {
                        for c in 0..cols {
                            let inst_x = origin.x + c * col_pitch_x + r * row_pitch_x;
                            let inst_y = origin.y + c * col_pitch_y + r * row_pitch_y;
                            let child_transform =
                                transform.compose(aref.strans.as_ref(), inst_x, inst_y);
                            flatten_cell(
                                child,
                                cell_map,
                                &child_transform,
                                layer,
                                datatype,
                                rects,
                                depth + 1,
                            );
                        }
                    }
                } else {
                    tracing::debug!("AREF to unknown cell '{}', skipping", aref.name);
                }
            }
            _ => {}
        }
    }
}

/// Read existing polygons on the artwork layer from a GDS file.
/// Flattens the cell hierarchy (SREFs/AREFs) to capture all geometry.
///
/// If `override_layer` is provided, reads from that (layer, datatype) pair
/// instead of the PDK's default artwork layer. This lets users exclude metal
/// from other layers (e.g. power straps on a different metal).
pub fn read_existing_metal(
    gds_path: &Path,
    pdk: &PdkConfig,
    cell_name: Option<&str>,
    override_layer: Option<(i16, i16)>,
) -> Result<Vec<Rect>> {
    let lib = load_gds(gds_path)?;

    let cell = if let Some(name) = cell_name {
        lib.structs.iter().find(|s| s.name == name).ok_or_else(|| {
            let available = format_cell_list(&lib.structs);
            anyhow::anyhow!(
                "Cell '{}' not found in GDS. Available cells: {}",
                name,
                available
            )
        })?
    } else {
        lib.structs
            .last()
            .ok_or_else(|| anyhow::anyhow!("No cells in GDS"))?
    };

    let (layer, datatype) =
        override_layer.unwrap_or((pdk.artwork_layer.gds_layer, pdk.artwork_layer.gds_datatype));

    // Build cell lookup map for hierarchy traversal
    let cell_map: HashMap<&str, &GdsStruct> =
        lib.structs.iter().map(|s| (s.name.as_str(), s)).collect();

    let mut rects = Vec::new();
    flatten_cell(
        cell,
        &cell_map,
        &Transform::identity(),
        layer,
        datatype,
        &mut rects,
        0,
    );

    tracing::info!(
        "Read {} existing metal rectangles from {} (flattened hierarchy)",
        rects.len(),
        gds_path.display()
    );

    Ok(rects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gds21::*;
    use tempfile::NamedTempFile;

    fn test_pdk() -> PdkConfig {
        PdkConfig::builtin("sky130").unwrap()
    }

    /// Helper: save a GdsLibrary to a temp file and return the path.
    fn save_lib(lib: &GdsLibrary) -> NamedTempFile {
        let f = NamedTempFile::new().unwrap();
        let path = f.path().to_path_buf();
        lib.save(&path).unwrap();
        f
    }

    fn make_rect_boundary(
        layer: i16,
        datatype: i16,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
    ) -> GdsElement {
        GdsElement::GdsBoundary(GdsBoundary {
            layer,
            datatype,
            xy: vec![
                GdsPoint::new(x0, y0),
                GdsPoint::new(x1, y0),
                GdsPoint::new(x1, y1),
                GdsPoint::new(x0, y1),
                GdsPoint::new(x0, y0),
            ],
            ..Default::default()
        })
    }

    #[test]
    fn test_sref_hierarchy_flattening() {
        let pdk = test_pdk();
        let layer = pdk.artwork_layer.gds_layer;
        let dt = pdk.artwork_layer.gds_datatype;

        // Child cell with a rect at (0,0)-(100,100)
        let mut child = GdsStruct::new("child");
        child
            .elems
            .push(make_rect_boundary(layer, dt, 0, 0, 100, 100));

        // Top cell with SREF to child at offset (1000, 2000)
        let mut top = GdsStruct::new("top");
        top.elems.push(GdsElement::GdsStructRef(GdsStructRef {
            name: "child".into(),
            xy: GdsPoint::new(1000, 2000),
            ..Default::default()
        }));

        let mut lib = GdsLibrary::new("test");
        lib.structs.push(child);
        lib.structs.push(top);
        let f = save_lib(&lib);

        let rects = read_existing_metal(f.path(), &pdk, Some("top"), None).unwrap();
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0], Rect::new(1000, 2000, 1100, 2100));
    }

    #[test]
    fn test_arbitrary_polygon_bounding_box() {
        let pdk = test_pdk();
        let layer = pdk.artwork_layer.gds_layer;
        let dt = pdk.artwork_layer.gds_datatype;

        // 8-point L-shaped polygon (not a simple rectangle)
        let mut cell = GdsStruct::new("top");
        cell.elems.push(GdsElement::GdsBoundary(GdsBoundary {
            layer,
            datatype: dt,
            xy: vec![
                GdsPoint::new(0, 0),
                GdsPoint::new(200, 0),
                GdsPoint::new(200, 100),
                GdsPoint::new(100, 100),
                GdsPoint::new(100, 200),
                GdsPoint::new(0, 200),
                GdsPoint::new(0, 0), // close - only 7 points, not 5
            ],
            ..Default::default()
        }));

        let mut lib = GdsLibrary::new("test");
        lib.structs.push(cell);
        let f = save_lib(&lib);

        let rects = read_existing_metal(f.path(), &pdk, Some("top"), None).unwrap();
        assert_eq!(rects.len(), 1);
        // Bounding box should encompass the whole L-shape
        assert_eq!(rects[0], Rect::new(0, 0, 200, 200));
    }

    #[test]
    fn test_path_element_with_width() {
        let pdk = test_pdk();
        let layer = pdk.artwork_layer.gds_layer;
        let dt = pdk.artwork_layer.gds_datatype;

        let mut cell = GdsStruct::new("top");
        cell.elems.push(GdsElement::GdsPath(GdsPath {
            layer,
            datatype: dt,
            xy: vec![GdsPoint::new(100, 100), GdsPoint::new(300, 100)],
            width: Some(40),
            ..Default::default()
        }));

        let mut lib = GdsLibrary::new("test");
        lib.structs.push(cell);
        let f = save_lib(&lib);

        let rects = read_existing_metal(f.path(), &pdk, Some("top"), None).unwrap();
        assert_eq!(rects.len(), 1);
        // half_w = 20, so bbox: (80, 80) to (320, 120)
        assert_eq!(rects[0], Rect::new(80, 80, 320, 120));
    }

    #[test]
    fn test_array_ref_2x2() {
        let pdk = test_pdk();
        let layer = pdk.artwork_layer.gds_layer;
        let dt = pdk.artwork_layer.gds_datatype;

        let mut child = GdsStruct::new("unit");
        child
            .elems
            .push(make_rect_boundary(layer, dt, 0, 0, 50, 50));

        // 2x2 array: origin (0,0), col pitch 100, row pitch 200
        let mut top = GdsStruct::new("top");
        top.elems.push(GdsElement::GdsArrayRef(GdsArrayRef {
            name: "unit".into(),
            xy: [
                GdsPoint::new(0, 0),   // origin
                GdsPoint::new(200, 0), // origin + 2*col_pitch_x
                GdsPoint::new(0, 400), // origin + 2*row_pitch_y
            ],
            cols: 2,
            rows: 2,
            ..Default::default()
        }));

        let mut lib = GdsLibrary::new("test");
        lib.structs.push(child);
        lib.structs.push(top);
        let f = save_lib(&lib);

        let rects = read_existing_metal(f.path(), &pdk, Some("top"), None).unwrap();
        assert_eq!(rects.len(), 4);

        // Sort for deterministic comparison
        let mut rects = rects;
        rects.sort_by_key(|r| (r.x0, r.y0));

        assert_eq!(rects[0], Rect::new(0, 0, 50, 50)); // (0,0)
        assert_eq!(rects[1], Rect::new(0, 200, 50, 250)); // (0,1)
        assert_eq!(rects[2], Rect::new(100, 0, 150, 50)); // (1,0)
        assert_eq!(rects[3], Rect::new(100, 200, 150, 250)); // (1,1)
    }

    #[test]
    fn test_recursion_depth_limit() {
        let pdk = test_pdk();
        let layer = pdk.artwork_layer.gds_layer;
        let dt = pdk.artwork_layer.gds_datatype;

        // Self-referencing cell (should not stack overflow)
        let mut cell = GdsStruct::new("loop_cell");
        cell.elems.push(make_rect_boundary(layer, dt, 0, 0, 10, 10));
        cell.elems.push(GdsElement::GdsStructRef(GdsStructRef {
            name: "loop_cell".into(),
            xy: GdsPoint::new(100, 0),
            ..Default::default()
        }));

        let mut lib = GdsLibrary::new("test");
        lib.structs.push(cell);
        let f = save_lib(&lib);

        // Should not panic or infinite-loop; just hits depth limit
        let rects = read_existing_metal(f.path(), &pdk, Some("loop_cell"), None).unwrap();
        // We get one rect per recursion level up to MAX_HIERARCHY_DEPTH
        assert_eq!(rects.len(), MAX_HIERARCHY_DEPTH);
    }

    #[test]
    fn test_rotation_90_degrees() {
        let pdk = test_pdk();
        let layer = pdk.artwork_layer.gds_layer;
        let dt = pdk.artwork_layer.gds_datatype;

        let mut child = GdsStruct::new("child");
        child
            .elems
            .push(make_rect_boundary(layer, dt, 0, 0, 100, 50));

        let mut top = GdsStruct::new("top");
        top.elems.push(GdsElement::GdsStructRef(GdsStructRef {
            name: "child".into(),
            xy: GdsPoint::new(0, 0),
            strans: Some(GdsStrans {
                reflected: false,
                abs_mag: false,
                abs_angle: false,
                mag: None,
                angle: Some(90.0),
            }),
            ..Default::default()
        }));

        let mut lib = GdsLibrary::new("test");
        lib.structs.push(child);
        lib.structs.push(top);
        let f = save_lib(&lib);

        let rects = read_existing_metal(f.path(), &pdk, Some("top"), None).unwrap();
        assert_eq!(rects.len(), 1);
        // 90-degree CCW rotation: (x,y) -> (-y,x)
        // Original corners: (0,0),(100,0),(100,50),(0,50)
        // Rotated: (0,0),(0,100),(-50,100),(-50,0)
        // Bounding box: (-50, 0, 0, 100)
        assert_eq!(rects[0], Rect::new(-50, 0, 0, 100));
    }

    #[test]
    fn test_rotation_180_degrees() {
        let t = Transform {
            offset_x: 0.0,
            offset_y: 0.0,
            angle_deg: 180.0,
            reflected: false,
            mag: 1.0,
        };
        let (x, y) = t.apply(&GdsPoint::new(100, 50));
        assert_eq!((x, y), (-100, -50));
    }

    #[test]
    fn test_rotation_270_degrees() {
        let t = Transform {
            offset_x: 0.0,
            offset_y: 0.0,
            angle_deg: 270.0,
            reflected: false,
            mag: 1.0,
        };
        // 270 CCW = 90 CW: (x,y) -> (y,-x)
        let (x, y) = t.apply(&GdsPoint::new(100, 50));
        assert_eq!((x, y), (50, -100));
    }

    #[test]
    fn test_reflect_x_axis() {
        let t = Transform {
            offset_x: 0.0,
            offset_y: 0.0,
            angle_deg: 0.0,
            reflected: true,
            mag: 1.0,
        };
        let (x, y) = t.apply(&GdsPoint::new(100, 50));
        assert_eq!((x, y), (100, -50));
    }

    #[test]
    fn test_write_gds_roundtrip() {
        let cell_name = "roundtrip_cell";
        let rects = vec![
            Rect::new(0, 0, 100, 100),
            Rect::new(200, 200, 400, 400),
            Rect::new(500, 0, 600, 50),
        ];
        let layers = vec![LayerRects {
            rects: &rects,
            layer: 72,
            datatype: 20,
        }];

        let tmp = NamedTempFile::with_suffix(".gds").unwrap();
        write_gds_multi(&layers, cell_name, tmp.path()).unwrap();

        // Read it back and verify
        let lib = GdsLibrary::load(tmp.path()).unwrap();
        assert_eq!(
            lib.structs.len(),
            1,
            "Expected exactly 1 cell in the library"
        );
        assert_eq!(
            lib.structs[0].name, cell_name,
            "Cell name should match what was written"
        );

        let boundary_count = lib.structs[0]
            .elems
            .iter()
            .filter(|e| matches!(e, GdsElement::GdsBoundary(_)))
            .count();
        assert_eq!(
            boundary_count,
            rects.len(),
            "Boundary count should match the number of rects written"
        );
    }

    #[test]
    fn test_merge_into_gds_roundtrip() {
        let pdk = test_pdk();
        let layer = pdk.artwork_layer.gds_layer;
        let dt = pdk.artwork_layer.gds_datatype;

        // Create a base GDS with one cell containing one boundary
        let mut base_cell = GdsStruct::new("merge_target");
        base_cell
            .elems
            .push(make_rect_boundary(layer, dt, 0, 0, 100, 100));
        let original_count = base_cell.elems.len();

        let mut base_lib = GdsLibrary::new("base");
        base_lib.structs.push(base_cell);
        let base_file = save_lib(&base_lib);

        // Merge additional rects into it
        let new_rects = vec![Rect::new(200, 200, 300, 300), Rect::new(400, 400, 500, 500)];
        let layers = vec![LayerRects {
            rects: &new_rects,
            layer,
            datatype: dt,
        }];

        let out_file = NamedTempFile::with_suffix(".gds").unwrap();
        merge_into_gds_multi(
            &layers,
            base_file.path(),
            out_file.path(),
            Some("merge_target"),
            0,
            0,
        )
        .unwrap();

        // Load the merged output and verify
        let merged_lib = GdsLibrary::load(out_file.path()).unwrap();
        let cell = merged_lib
            .structs
            .iter()
            .find(|s| s.name == "merge_target")
            .expect("merge_target cell should exist in merged output");

        let boundary_count = cell
            .elems
            .iter()
            .filter(|e| matches!(e, GdsElement::GdsBoundary(_)))
            .count();
        assert_eq!(
            boundary_count,
            original_count + new_rects.len(),
            "Merged file should have original + new boundaries"
        );
    }

    #[test]
    fn test_write_gds_single_layer() {
        let pdk = test_pdk();
        let rects = vec![Rect::new(0, 0, 1000, 1000), Rect::new(2000, 0, 3000, 500)];

        let tmp = NamedTempFile::with_suffix(".gds").unwrap();
        write_gds(&rects, &pdk, "single_layer_cell", tmp.path()).unwrap();

        // Verify the file exists and can be loaded
        assert!(
            tmp.path().exists(),
            "Output GDS file should exist at {}",
            tmp.path().display()
        );

        let lib = GdsLibrary::load(tmp.path()).unwrap();
        assert_eq!(lib.structs.len(), 1, "Should have exactly 1 cell");
        assert_eq!(
            lib.structs[0].name, "single_layer_cell",
            "Cell name should match"
        );

        let boundary_count = lib.structs[0]
            .elems
            .iter()
            .filter(|e| matches!(e, GdsElement::GdsBoundary(_)))
            .count();
        assert_eq!(
            boundary_count,
            rects.len(),
            "Should have written {} boundaries",
            rects.len()
        );
    }
}
