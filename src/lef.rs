use crate::pdk::PdkConfig;
use crate::polygon::{bounding_box, Rect};
use anyhow::{Context, Result};
use std::io::Write;
use std::path::Path;

/// Write a minimal LEF MACRO file for the artwork block.
///
/// This allows OpenLane/OpenROAD to treat the artwork as a placeable macro.
/// The LEF contains:
/// - MACRO definition with CLASS BLOCK
/// - SIZE from bounding box
/// - OBS (obstruction) on the artwork layer
pub fn write_lef(rects: &[Rect], pdk: &PdkConfig, cell_name: &str, output: &Path) -> Result<()> {
    let bb = bounding_box(rects).unwrap_or(Rect::new(0, 0, 0, 0));
    let dbu = pdk.pdk.db_units_per_um as f64;
    let width_um = bb.width() as f64 / dbu;
    let height_um = bb.height() as f64 / dbu;

    let layer_name = &pdk.artwork_layer.name;

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
    writeln!(f, "    LAYER {layer_name} ;")?;
    writeln!(f, "      RECT 0.000 0.000 {width_um:.3} {height_um:.3} ;")?;
    writeln!(f, "  END")?;
    writeln!(f, "END {cell_name}")?;
    writeln!(f)?;
    writeln!(f, "END LIBRARY")?;

    tracing::info!(
        "Wrote LEF macro '{}' ({:.1} x {:.1} um) to {}",
        cell_name,
        width_um,
        height_um,
        output.display()
    );

    Ok(())
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
