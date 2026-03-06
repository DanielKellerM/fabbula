use crate::pdk::PdkConfig;
use crate::polygon::Rect;
use anyhow::{Context, Result};
use std::io::Write;
use std::path::Path;

/// Layer entry for multi-layer LEF output.
pub struct LefLayer<'a> {
    pub rects: &'a [Rect],
    pub layer_name: &'a str,
}

/// Write a minimal LEF MACRO file with multiple artwork layers.
pub fn write_lef_multi(
    layers: &[LefLayer],
    pdk: &PdkConfig,
    cell_name: &str,
    output: &Path,
) -> Result<()> {
    let all_rects: Vec<&Rect> = layers.iter().flat_map(|l| l.rects.iter()).collect();
    let bb = if let Some(bb) = bounding_box_refs(&all_rects) {
        bb
    } else {
        Rect::new(0, 0, 0, 0)
    };
    let dbu = pdk.pdk.db_units_per_um as f64;
    let width_um = bb.width() as f64 / dbu;
    let height_um = bb.height() as f64 / dbu;

    let mut f = std::fs::File::create(output)
        .with_context(|| format!("Failed to create LEF file: {}", output.display()))?;

    writeln!(f, "VERSION 5.8 ;")?;
    writeln!(f, "BUSBITCHARS \"[]\" ;")?;
    writeln!(f, "DIVIDERCHAR \"/\" ;")?;
    writeln!(f)?;
    writeln!(f, "MACRO {cell_name}")?;
    writeln!(f, "  CLASS BLOCK ;")?;
    writeln!(f, "  ORIGIN 0 0 ;")?;
    writeln!(f, "  SIZE {width_um:.3} BY {height_um:.3} ;")?;
    writeln!(f, "  SYMMETRY X Y ;")?;
    writeln!(f, "  OBS")?;
    for layer in layers {
        writeln!(f, "    LAYER {} ;", layer.layer_name)?;
        writeln!(f, "      RECT 0.000 0.000 {width_um:.3} {height_um:.3} ;")?;
    }
    writeln!(f, "  END")?;
    writeln!(f, "END {cell_name}")?;
    writeln!(f)?;
    writeln!(f, "END LIBRARY")?;

    tracing::info!(
        "Wrote LEF macro '{}' ({:.1} x {:.1} um, {} layers) to {}",
        cell_name,
        width_um,
        height_um,
        layers.len(),
        output.display()
    );

    Ok(())
}

/// Write a minimal LEF MACRO file for the artwork block (single layer).
pub fn write_lef(rects: &[Rect], pdk: &PdkConfig, cell_name: &str, output: &Path) -> Result<()> {
    write_lef_multi(
        &[LefLayer {
            rects,
            layer_name: &pdk.artwork_layer.name,
        }],
        pdk,
        cell_name,
        output,
    )
}

fn bounding_box_refs(rects: &[&Rect]) -> Option<Rect> {
    let first = **rects.first()?;
    Some(rects[1..].iter().fold(first, |bb, r| Rect {
        x0: bb.x0.min(r.x0),
        y0: bb.y0.min(r.y0),
        x1: bb.x1.max(r.x1),
        y1: bb.y1.max(r.y1),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_lef() {
        let pdk = PdkConfig::builtin("sky130").unwrap();
        let rects = vec![Rect::new(0, 0, 1000, 1000), Rect::new(2000, 0, 3000, 1000)];
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.lef");
        write_lef(&rects, &pdk, "test_art", &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("MACRO test_art"));
        assert!(content.contains("CLASS BLOCK"));
        assert!(content.contains("SIZE"));
        assert!(content.contains("OBS"));
        assert!(content.contains(&pdk.artwork_layer.name));
        assert!(content.contains("END test_art"));
        assert!(content.contains("END LIBRARY"));
    }
}
