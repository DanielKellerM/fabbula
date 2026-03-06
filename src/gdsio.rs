use crate::pdk::PdkConfig;
use crate::polygon::Rect;
use anyhow::Result;
use gds21::{GdsBoundary, GdsLibrary, GdsPoint, GdsStruct};
use std::path::Path;

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

/// Write polygons to a new GDSII file
pub fn write_gds(rects: &[Rect], pdk: &PdkConfig, cell_name: &str, output: &Path) -> Result<()> {
    let mut lib = GdsLibrary::new("fabbula");
    // Use default GdsUnits (1e-9 db, 1e-6 user) — standard for nm-resolution layouts

    let mut cell = GdsStruct::new(cell_name);

    let layer = pdk.artwork_layer.gds_layer;
    let datatype = pdk.artwork_layer.gds_datatype;

    for rect in rects {
        let boundary = make_boundary(rect, layer, datatype, 0, 0);
        cell.elems.push(gds21::GdsElement::GdsBoundary(boundary));
    }

    lib.structs.push(cell);

    lib.save(output)
        .map_err(|e| anyhow::anyhow!("Failed to write GDSII {}: {:?}", output.display(), e))?;

    tracing::info!(
        "Wrote {} polygons to {} (cell: {}, layer: {}/{})",
        rects.len(),
        output.display(),
        cell_name,
        layer,
        datatype
    );

    Ok(())
}

/// Merge artwork polygons into an existing GDSII file.
/// Reads the input GDS, adds artwork to the top cell, writes output.
pub fn merge_into_gds(
    rects: &[Rect],
    pdk: &PdkConfig,
    input_gds: &Path,
    output_gds: &Path,
    target_cell: Option<&str>,
    offset_x: i32,
    offset_y: i32,
) -> Result<()> {
    let mut lib = GdsLibrary::load(input_gds)
        .map_err(|e| anyhow::anyhow!("Failed to read GDSII {}: {:?}", input_gds.display(), e))?;

    // Find target cell (default: last struct, which is typically the top cell)
    let cell = if let Some(name) = target_cell {
        lib.structs
            .iter_mut()
            .find(|s| s.name == name)
            .ok_or_else(|| anyhow::anyhow!("Cell '{}' not found in GDS", name))?
    } else {
        lib.structs
            .last_mut()
            .ok_or_else(|| anyhow::anyhow!("No cells in input GDS"))?
    };

    let layer = pdk.artwork_layer.gds_layer;
    let datatype = pdk.artwork_layer.gds_datatype;

    for rect in rects {
        let boundary = make_boundary(rect, layer, datatype, offset_x, offset_y);
        cell.elems.push(gds21::GdsElement::GdsBoundary(boundary));
    }

    lib.save(output_gds)
        .map_err(|e| anyhow::anyhow!("Failed to write GDSII {}: {:?}", output_gds.display(), e))?;

    tracing::info!(
        "Merged {} artwork polygons into {} -> {}",
        rects.len(),
        input_gds.display(),
        output_gds.display()
    );

    Ok(())
}

/// Read existing polygons on the artwork layer from a GDS file.
/// Used to avoid overlapping with existing top-metal structures.
pub fn read_existing_metal(
    gds_path: &Path,
    pdk: &PdkConfig,
    cell_name: Option<&str>,
) -> Result<Vec<Rect>> {
    let lib = GdsLibrary::load(gds_path)
        .map_err(|e| anyhow::anyhow!("Failed to read GDSII {}: {:?}", gds_path.display(), e))?;

    let cell = if let Some(name) = cell_name {
        lib.structs
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| anyhow::anyhow!("Cell '{}' not found", name))?
    } else {
        lib.structs
            .last()
            .ok_or_else(|| anyhow::anyhow!("No cells in GDS"))?
    };

    let layer = pdk.artwork_layer.gds_layer;
    let datatype = pdk.artwork_layer.gds_datatype;

    let rects: Vec<Rect> = cell
        .elems
        .iter()
        .filter_map(|elem| match elem {
            gds21::GdsElement::GdsBoundary(b)
                if b.layer == layer && b.datatype == datatype && b.xy.len() == 5 =>
            {
                let (x0, y0, x1, y1) = b.xy.iter().fold(
                    (i32::MAX, i32::MAX, i32::MIN, i32::MIN),
                    |(x0, y0, x1, y1), p| (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)),
                );
                Some(Rect::new(x0, y0, x1, y1))
            }
            _ => None,
        })
        .collect();

    tracing::info!(
        "Read {} existing metal rectangles from {}",
        rects.len(),
        gds_path.display()
    );

    Ok(rects)
}
