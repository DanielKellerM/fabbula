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
    /// Minimum enclosed area (donut holes) in µm²
    #[serde(default)]
    pub min_enclosed_area: f64,
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
            self.grid.manufacturing_grid_um > 0.0,
            "Manufacturing grid must be positive"
        );
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
}
