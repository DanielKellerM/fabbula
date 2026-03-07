// Copyright 2026 Daniel Keller <daniel.keller.m@gmail.com>
// Licensed under the Apache License, Version 2.0.
// SPDX-License-Identifier: Apache-2.0

//! PDK configuration loading and built-in PDK support.
//!
//! Loads process design kit parameters from TOML files or built-in definitions.
//! Provides unit conversion ([`PdkConfig::um_to_dbu`]) and grid snapping.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

/// Convert micrometers to database units given the scale factor.
pub(crate) fn um_to_dbu(um: f64, db_units_per_um: u32) -> i32 {
    (um * db_units_per_um as f64).round() as i32
}

/// PDK metadata
#[derive(Debug, Clone, Deserialize)]
pub struct PdkInfo {
    pub name: String,
    pub description: String,
    pub node_nm: u32,
    pub db_units_per_um: u32,
}

/// The GDS layer to place artwork on
#[derive(Debug, Clone, Deserialize)]
pub struct ArtworkLayerConfig {
    pub name: String,
    pub gds_layer: i16,
    pub gds_datatype: i16,
    #[serde(default)]
    pub purpose: String,
}

/// DRC constraints for the artwork layer — the key differentiator
#[derive(Debug, Clone, Deserialize)]
pub struct DrcRules {
    /// Minimum polygon width in µm
    pub min_width: f64,
    /// Minimum spacing between polygons in µm
    pub min_spacing: f64,
    /// Minimum polygon area in µm²
    #[serde(default)]
    pub min_area: f64,
    /// Minimum metal density (0.0–1.0)
    #[serde(default)]
    pub density_min: f64,
    /// Maximum metal density (0.0–1.0)
    #[serde(default = "default_density_max")]
    pub density_max: f64,
    /// Density check window size in µm
    #[serde(default = "default_density_window")]
    pub density_window_um: f64,
    /// Maximum metal width in µm before slotting is needed (Cu layers)
    #[serde(default)]
    pub max_width: Option<f64>,
    /// Width threshold in µm at which a rect is considered "wide/huge" metal
    #[serde(default)]
    pub wide_metal_threshold: Option<f64>,
    /// Spacing required between wide/huge metal features in µm
    #[serde(default)]
    pub wide_metal_spacing: Option<f64>,
}

impl DrcRules {
    /// Return the effective minimum spacing, accounting for wide-metal rules.
    ///
    /// When wide_metal_threshold is <= 2 * min_width, merged rectangles will
    /// inevitably exceed the threshold, so we must use wide_metal_spacing to
    /// maintain DRC-clean-by-construction guarantees.
    pub fn effective_spacing(&self) -> f64 {
        if let (Some(threshold), Some(wide_spacing)) =
            (self.wide_metal_threshold, self.wide_metal_spacing)
            && threshold <= self.min_width * 2.0
        {
            return wide_spacing.max(self.min_spacing);
        }
        self.min_spacing
    }
}

fn default_density_max() -> f64 {
    0.80
}
fn default_density_window() -> f64 {
    500.0
}

/// Grid snapping rules
#[derive(Debug, Clone, Deserialize)]
pub struct GridConfig {
    pub manufacturing_grid_um: f64,
    #[serde(default)]
    pub placement_grid_um: f64,
}

/// A metal layer in the stack (for reference / rendering)
#[derive(Debug, Clone, Deserialize)]
pub struct MetalLayerInfo {
    pub name: String,
    pub gds_layer: i16,
    pub gds_datatype: i16,
}

/// An artwork layer profile combining layer config and DRC rules.
/// Used by multi-layer color workflows.
#[derive(Debug, Clone, Deserialize)]
pub struct ArtworkLayerProfile {
    pub name: String,
    pub gds_layer: i16,
    pub gds_datatype: i16,
    #[serde(default)]
    pub purpose: String,
    /// Color channel mapping for channel extraction mode ("red", "green", "blue")
    #[serde(default)]
    pub color: Option<String>,
    pub drc: DrcRules,
}

/// Full PDK configuration
#[derive(Debug, Clone, Deserialize)]
pub struct PdkConfig {
    pub pdk: PdkInfo,
    pub artwork_layer: ArtworkLayerConfig,
    pub artwork_layer_alt: Option<ArtworkLayerConfig>,
    pub drc: DrcRules,
    #[serde(default)]
    pub drc_alt: Option<DrcRules>,
    pub grid: GridConfig,
    #[serde(default)]
    pub metal_stack: Vec<MetalLayerInfo>,
    /// Optional multi-layer artwork profiles (new format)
    #[serde(default)]
    pub artwork_layers: Option<Vec<ArtworkLayerProfile>>,
}

impl PdkConfig {
    /// Load a PDK config from a TOML file
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read PDK config: {}", path.display()))?;
        let config: PdkConfig = toml::from_str(&content)
            .with_context(|| format!("Failed to parse PDK config: {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    /// Load a built-in PDK by name
    pub fn builtin(name: &str) -> Result<Self> {
        let toml_str = match name {
            "sky130" => include_str!("../pdks/sky130.toml"),
            "ihp_sg13g2" | "ihp" | "sg13g2" => include_str!("../pdks/ihp_sg13g2.toml"),
            "gf180mcu" | "gf180" => include_str!("../pdks/gf180mcu.toml"),
            "freepdk45" => include_str!("../pdks/freepdk45.toml"),
            "asap7" => include_str!("../pdks/asap7.toml"),
            "fabbula2" => include_str!("../pdks/fabbula2.toml"),
            other => anyhow::bail!(
                "Unknown built-in PDK '{}'. Available: sky130, ihp_sg13g2, gf180mcu, freepdk45, asap7, fabbula2",
                other
            ),
        };
        let config: PdkConfig = toml::from_str(toml_str)?;
        config.validate()?;
        Ok(config)
    }

    /// List available built-in PDKs
    pub fn list_builtins() -> &'static [&'static str] {
        &[
            "sky130",
            "ihp_sg13g2",
            "gf180mcu",
            "freepdk45",
            "asap7",
            "fabbula2",
        ]
    }

    /// Return the DRC rules for the active layer.
    /// When `use_alt` is true and `drc_alt` exists, return alt rules;
    /// otherwise fall back to the primary `drc` rules.
    pub fn active_drc(&self, use_alt: bool) -> &DrcRules {
        if use_alt {
            self.drc_alt.as_ref().unwrap_or(&self.drc)
        } else {
            &self.drc
        }
    }

    /// Return artwork layer profiles for multi-layer workflows.
    ///
    /// If `artwork_layers` is set in the TOML, returns those directly.
    /// Otherwise, builds profiles from the legacy `artwork_layer` + `drc` fields,
    /// including `artwork_layer_alt` + `drc_alt` if present.
    pub fn layer_profiles(&self) -> Vec<ArtworkLayerProfile> {
        if let Some(ref layers) = self.artwork_layers {
            return layers.clone();
        }
        let mut profiles = vec![ArtworkLayerProfile {
            name: self.artwork_layer.name.clone(),
            gds_layer: self.artwork_layer.gds_layer,
            gds_datatype: self.artwork_layer.gds_datatype,
            purpose: self.artwork_layer.purpose.clone(),
            color: None,
            drc: self.drc.clone(),
        }];
        if let (Some(alt_layer), Some(alt_drc)) = (&self.artwork_layer_alt, &self.drc_alt) {
            profiles.push(ArtworkLayerProfile {
                name: alt_layer.name.clone(),
                gds_layer: alt_layer.gds_layer,
                gds_datatype: alt_layer.gds_datatype,
                purpose: alt_layer.purpose.clone(),
                color: None,
                drc: alt_drc.clone(),
            });
        }
        profiles
    }

    /// Compute pixel pitch for a given DRC rule set.
    /// Uses effective_spacing to account for wide-metal rules.
    pub fn pixel_pitch_um_for_drc(&self, drc: &DrcRules) -> f64 {
        let pw = self.snap_to_grid(drc.min_width);
        let gap = self.snap_to_grid(drc.effective_spacing());
        pw + gap
    }

    fn validate(&self) -> Result<()> {
        Self::validate_drc_rules(&self.drc, "drc")?;
        if let Some(ref alt) = self.drc_alt {
            Self::validate_drc_rules(alt, "drc_alt")?;
        }
        if let Some(ref layers) = self.artwork_layers {
            for (i, profile) in layers.iter().enumerate() {
                Self::validate_drc_rules(&profile.drc, &format!("artwork_layers[{}].drc", i))?;
            }
        }
        anyhow::ensure!(
            self.pdk.db_units_per_um > 0,
            "db_units_per_um must be positive"
        );
        anyhow::ensure!(
            self.grid.manufacturing_grid_um > 0.0,
            "Manufacturing grid must be positive"
        );
        // Check artwork_layer vs artwork_layer_alt GDS layer collision
        if let Some(ref alt) = self.artwork_layer_alt
            && self.artwork_layer.gds_layer == alt.gds_layer
            && self.artwork_layer.gds_datatype == alt.gds_datatype
        {
            anyhow::bail!(
                "artwork_layer and artwork_layer_alt have the same GDS layer/datatype ({}/{})",
                self.artwork_layer.gds_layer,
                self.artwork_layer.gds_datatype
            );
        }
        Ok(())
    }

    fn validate_drc_rules(rules: &DrcRules, section: &str) -> Result<()> {
        anyhow::ensure!(
            rules.min_width > 0.0,
            "{} min_width must be positive, got {}",
            section,
            rules.min_width
        );
        anyhow::ensure!(
            rules.min_spacing > 0.0,
            "{} min_spacing must be positive, got {}",
            section,
            rules.min_spacing
        );
        anyhow::ensure!(
            rules.density_max > 0.0 && rules.density_max <= 1.0,
            "{} density_max must be in (0, 1]",
            section
        );
        if let Some(max_w) = rules.max_width {
            anyhow::ensure!(
                max_w > rules.min_width,
                "{} max_width ({}) must be > min_width ({})",
                section,
                max_w,
                rules.min_width
            );
        }
        anyhow::ensure!(
            rules.min_area >= 0.0,
            "{} min_area must be non-negative, got {}",
            section,
            rules.min_area
        );
        anyhow::ensure!(
            rules.density_min >= 0.0 && rules.density_min <= 1.0,
            "{} density_min must be in [0, 1], got {}",
            section,
            rules.density_min
        );
        anyhow::ensure!(
            rules.density_min <= rules.density_max,
            "{} density_min ({}) must be <= density_max ({})",
            section,
            rules.density_min,
            rules.density_max
        );
        if let Some(wide_s) = rules.wide_metal_spacing {
            anyhow::ensure!(
                wide_s >= rules.min_spacing,
                "{} wide_metal_spacing ({}) must be >= min_spacing ({})",
                section,
                wide_s,
                rules.min_spacing
            );
        }
        Ok(())
    }

    /// Convert µm to database units
    pub fn um_to_dbu(&self, um: f64) -> i32 {
        um_to_dbu(um, self.pdk.db_units_per_um)
    }

    /// Snap a µm value to the manufacturing grid
    pub fn snap_to_grid(&self, um: f64) -> f64 {
        let grid = self.grid.manufacturing_grid_um;
        (um / grid).round() * grid
    }

    /// Minimum pixel size in µm that satisfies both min_width and spacing rules.
    /// Each "pixel" in the artwork maps to a square of this size.
    /// This is what makes DRC-clean output possible: if every polygon
    /// is at least min_width wide and every gap satisfies effective_spacing,
    /// all width/spacing rules are satisfied by construction.
    pub fn pixel_pitch_um(&self) -> f64 {
        let pw = self.snap_to_grid(self.drc.min_width);
        let gap = self.snap_to_grid(self.drc.effective_spacing());
        pw + gap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_sky130() {
        let pdk = PdkConfig::builtin("sky130").unwrap();
        assert_eq!(pdk.pdk.name, "sky130");
        assert_eq!(pdk.artwork_layer.gds_layer, 72);
        assert!(pdk.drc.min_width > 0.0);
    }

    #[test]
    fn test_load_ihp() {
        let pdk = PdkConfig::builtin("ihp_sg13g2").unwrap();
        assert_eq!(pdk.artwork_layer.gds_layer, 134);
    }

    #[test]
    fn test_load_gf180() {
        let pdk = PdkConfig::builtin("gf180mcu").unwrap();
        assert_eq!(pdk.artwork_layer.gds_layer, 81);
    }

    #[test]
    fn test_load_freepdk45() {
        let pdk = PdkConfig::builtin("freepdk45").unwrap();
        assert_eq!(pdk.pdk.name, "freepdk45");
        assert_eq!(pdk.artwork_layer.gds_layer, 29);
        assert!(pdk.drc.min_width > 0.0);
    }

    #[test]
    fn test_load_asap7() {
        let pdk = PdkConfig::builtin("asap7").unwrap();
        assert_eq!(pdk.pdk.name, "asap7");
        assert_eq!(pdk.artwork_layer.gds_layer, 90);
        assert!(pdk.drc.min_width > 0.0);
    }

    #[test]
    fn test_load_fabbula2() {
        let pdk = PdkConfig::builtin("fabbula2").unwrap();
        assert_eq!(pdk.pdk.name, "fabbula2");
        assert_eq!(pdk.pdk.node_nm, 2);
        assert_eq!(pdk.artwork_layer.gds_layer, 150);
        assert!(pdk.drc.min_width > 0.0);
    }

    #[test]
    fn test_snap_to_grid() {
        let pdk = PdkConfig::builtin("sky130").unwrap();
        let snapped = pdk.snap_to_grid(1.603);
        assert!((snapped - 1.605).abs() < 1e-9 || (snapped - 1.600).abs() < 1e-9);
    }

    #[test]
    fn test_effective_spacing_no_wide_metal() {
        let pdk = PdkConfig::builtin("sky130").unwrap();
        // sky130 primary drc has no wide_metal_threshold
        assert!(pdk.drc.wide_metal_threshold.is_none());
        assert!((pdk.drc.effective_spacing() - pdk.drc.min_spacing).abs() < 1e-9);
    }

    #[test]
    fn test_effective_spacing_wide_metal_triggers() {
        let mut pdk = PdkConfig::builtin("sky130").unwrap();
        // Set threshold <= 2 * min_width so it triggers
        pdk.drc.wide_metal_threshold = Some(pdk.drc.min_width * 2.0);
        pdk.drc.wide_metal_spacing = Some(pdk.drc.min_spacing + 1.0);
        let expected = pdk.drc.min_spacing + 1.0;
        assert!((pdk.drc.effective_spacing() - expected).abs() < 1e-9);
    }

    #[test]
    fn test_effective_spacing_wide_metal_no_trigger() {
        let mut pdk = PdkConfig::builtin("sky130").unwrap();
        // Set threshold > 2 * min_width so it does NOT trigger
        pdk.drc.wide_metal_threshold = Some(pdk.drc.min_width * 2.0 + 1.0);
        pdk.drc.wide_metal_spacing = Some(pdk.drc.min_spacing + 5.0);
        assert!((pdk.drc.effective_spacing() - pdk.drc.min_spacing).abs() < 1e-9);
    }

    #[test]
    fn test_validate_rejects_zero_min_width() {
        let toml_str = r#"
[pdk]
name = "bad"
description = "bad pdk"
node_nm = 130
db_units_per_um = 1000

[artwork_layer]
name = "met1"
gds_layer = 1
gds_datatype = 0

[drc]
min_width = 0.0
min_spacing = 0.5

[grid]
manufacturing_grid_um = 0.005
"#;
        let config: PdkConfig = toml::from_str(toml_str).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_bad_density_max() {
        // density_max = 0 should fail
        let toml_zero = r#"
[pdk]
name = "bad"
description = "bad pdk"
node_nm = 130
db_units_per_um = 1000

[artwork_layer]
name = "met1"
gds_layer = 1
gds_datatype = 0

[drc]
min_width = 1.0
min_spacing = 0.5
density_max = 0.0

[grid]
manufacturing_grid_um = 0.005
"#;
        let config: PdkConfig = toml::from_str(toml_zero).unwrap();
        assert!(config.validate().is_err());

        // density_max = 1.5 should also fail
        let toml_over = r#"
[pdk]
name = "bad"
description = "bad pdk"
node_nm = 130
db_units_per_um = 1000

[artwork_layer]
name = "met1"
gds_layer = 1
gds_datatype = 0

[drc]
min_width = 1.0
min_spacing = 0.5
density_max = 1.5

[grid]
manufacturing_grid_um = 0.005
"#;
        let config_over: PdkConfig = toml::from_str(toml_over).unwrap();
        assert!(config_over.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_max_width_below_min() {
        let toml_str = r#"
[pdk]
name = "bad"
description = "bad pdk"
node_nm = 130
db_units_per_um = 1000

[artwork_layer]
name = "met1"
gds_layer = 1
gds_datatype = 0

[drc]
min_width = 1.0
min_spacing = 0.5
max_width = 0.5

[grid]
manufacturing_grid_um = 0.005
"#;
        let config: PdkConfig = toml::from_str(toml_str).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_layer_profiles_single() {
        let pdk = PdkConfig::builtin("sky130").unwrap();
        // sky130 default has artwork_layer but we need to check without alt
        let mut pdk_no_alt = pdk.clone();
        pdk_no_alt.artwork_layer_alt = None;
        pdk_no_alt.drc_alt = None;
        let profiles = pdk_no_alt.layer_profiles();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, pdk_no_alt.artwork_layer.name);
        assert_eq!(profiles[0].gds_layer, pdk_no_alt.artwork_layer.gds_layer);
    }

    #[test]
    fn test_layer_profiles_with_alt() {
        let pdk = PdkConfig::builtin("sky130").unwrap();
        // sky130 has both artwork_layer_alt and drc_alt
        assert!(pdk.artwork_layer_alt.is_some());
        assert!(pdk.drc_alt.is_some());
        let profiles = pdk.layer_profiles();
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].name, "met5");
        assert_eq!(profiles[1].name, "met4");
    }

    #[test]
    fn test_from_file_valid() {
        use std::io::Write;
        let toml_str = r#"
[pdk]
name = "test"
description = "test pdk"
node_nm = 130
db_units_per_um = 1000

[artwork_layer]
name = "met5"
gds_layer = 72
gds_datatype = 20

[drc]
min_width = 1.0
min_spacing = 0.5

[grid]
manufacturing_grid_um = 0.005
"#;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(tmp, "{}", toml_str).unwrap();
        let config = PdkConfig::from_file(tmp.path()).unwrap();
        assert_eq!(config.pdk.name, "test");
        assert_eq!(config.pdk.db_units_per_um, 1000);
        assert!((config.drc.min_width - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_from_file_invalid() {
        use std::io::Write;
        let bad_toml = "this is not valid toml [[[";
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(tmp, "{}", bad_toml).unwrap();
        assert!(PdkConfig::from_file(tmp.path()).is_err());
    }

    #[test]
    fn test_um_to_dbu() {
        let pdk = PdkConfig::builtin("sky130").unwrap();
        // sky130 has db_units_per_um = 1000, so 1.0 um = 1000 dbu
        assert_eq!(pdk.um_to_dbu(1.0), 1000);
        assert_eq!(pdk.um_to_dbu(0.5), 500);
        assert_eq!(pdk.um_to_dbu(1.6), 1600);
        // Also test the free function directly
        assert_eq!(um_to_dbu(1.0, 1000), 1000);
        assert_eq!(um_to_dbu(2.5, 2000), 5000);
    }

    #[test]
    fn test_pixel_pitch_um_for_drc() {
        let pdk = PdkConfig::builtin("sky130").unwrap();
        let pitch = pdk.pixel_pitch_um_for_drc(&pdk.drc);
        let expected =
            pdk.snap_to_grid(pdk.drc.min_width) + pdk.snap_to_grid(pdk.drc.effective_spacing());
        assert!((pitch - expected).abs() < 1e-9);
        // For sky130 met5: min_width=1.6, min_spacing=1.6, no wide metal
        // snap_to_grid(1.6) = 1.6 (already on 0.005 grid)
        // So pitch should be 3.2
        assert!((pitch - 3.2).abs() < 1e-9);
    }
}
