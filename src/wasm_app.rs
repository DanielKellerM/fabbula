// Copyright 2026 Daniel Keller <daniel.keller.m@gmail.com>
// Licensed under the Apache License, Version 2.0.
// SPDX-License-Identifier: Apache-2.0

use crate::artwork::{ArtworkBitmap, DitherMode, OverlayPosition, ThresholdMode, bitmap_from_rgba};
use crate::drc::check_drc;
use crate::gdsio::{LayerRects, write_gds_multi_to_bytes};
use crate::generation::generate_layer_polygons;
use crate::pdk::PdkConfig;
use crate::polygon::{PixelPlacement, PolygonStrategy, Rect, bounding_box};
use crate::qr::{EcLevel, render_qr};
use crate::text::render_text;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GenerateRequest {
    pub(crate) pixels: Vec<u8>,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pdk_name: Option<String>,
    pub(crate) custom_pdk_toml: Option<String>,
    pub(crate) strategy: Option<String>,
    pub(crate) separated: Option<bool>,
    pub(crate) threshold: Option<String>,
    pub(crate) invert: Option<bool>,
    pub(crate) dither: Option<bool>,
    pub(crate) rotate: Option<u32>,
    pub(crate) flip: Option<String>,
    pub(crate) no_check_drc: Option<bool>,
    pub(crate) no_density_enforce: Option<bool>,
    pub(crate) force: Option<bool>,
    pub(crate) text: Option<String>,
    pub(crate) text_position: Option<String>,
    pub(crate) text_scale: Option<u32>,
    pub(crate) qr: Option<String>,
    pub(crate) qr_position: Option<String>,
    pub(crate) qr_module_size: Option<u32>,
    pub(crate) qr_ec_level: Option<String>,
    pub(crate) overlay_margin: Option<u32>,
    pub(crate) cell_name: Option<String>,
    pub(crate) library_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct GenerateResponse {
    pub(crate) rects: Vec<[i32; 4]>,
    pub(crate) violations: Vec<ViolationResponse>,
    pub(crate) stats: StatsResponse,
}

#[derive(Debug, Serialize)]
pub(crate) struct ViolationResponse {
    pub(crate) rule: String,
    pub(crate) rect_index: u32,
    pub(crate) other_index: u32,
    pub(crate) value: i64,
    pub(crate) limit: i64,
    pub(crate) location: [i32; 2],
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct StatsResponse {
    pub(crate) polygon_count: usize,
    pub(crate) width_dbu: i32,
    pub(crate) height_dbu: i32,
    pub(crate) width_um: f64,
    pub(crate) height_um: f64,
    pub(crate) bitmap_width: u32,
    pub(crate) bitmap_height: u32,
    pub(crate) bitmap_density: f64,
}

#[derive(Debug, Serialize)]
pub(crate) struct PdkValidationInfo {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) node_nm: u32,
    pub(crate) db_units_per_um: u32,
    pub(crate) pixel_pitch_um: f64,
    pub(crate) layers: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct PdkValidationResponse {
    pub(crate) valid: bool,
    pub(crate) error: Option<String>,
    pub(crate) info: Option<PdkValidationInfo>,
}

#[derive(Debug, Serialize)]
pub(crate) struct BuiltinPdkInfo {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) node_nm: u32,
    pub(crate) min_width: f64,
    pub(crate) min_spacing: f64,
}

pub(crate) struct PipelineOutput {
    pub(crate) pdk: PdkConfig,
    pub(crate) rects: Vec<Rect>,
    pub(crate) violations: Vec<crate::drc::DrcViolation>,
    pub(crate) stats: StatsResponse,
}

fn parse_strategy(s: Option<&str>) -> anyhow::Result<PolygonStrategy> {
    match s.unwrap_or("greedy-merge") {
        "pixel-rects" | "pixel_rects" | "pixelrects" => Ok(PolygonStrategy::PixelRects),
        "row-merge" | "row_merge" | "rowmerge" => Ok(PolygonStrategy::RowMerge),
        "greedy-merge" | "greedy_merge" | "greedymerge" => Ok(PolygonStrategy::GreedyMerge),
        "histogram-merge" | "histogram_merge" | "histogrammerge" => {
            Ok(PolygonStrategy::HistogramMerge)
        }
        other => anyhow::bail!("Invalid strategy '{}'", other),
    }
}

fn parse_threshold(s: Option<&str>) -> anyhow::Result<ThresholdMode> {
    let s = s.unwrap_or("128");
    if s.eq_ignore_ascii_case("otsu") {
        Ok(ThresholdMode::Otsu)
    } else if s.eq_ignore_ascii_case("auto") {
        Ok(ThresholdMode::Auto)
    } else if s.eq_ignore_ascii_case("alpha") {
        Ok(ThresholdMode::Alpha(128))
    } else if let Ok(v) = s.parse::<u8>() {
        Ok(ThresholdMode::Luminance(v))
    } else {
        anyhow::bail!(
            "Invalid threshold '{}': expected 'otsu', 'auto', 'alpha', or 0-255",
            s
        )
    }
}

fn parse_flip(s: Option<&str>) -> anyhow::Result<Option<&str>> {
    match s {
        None => Ok(None),
        Some(v) if v.eq_ignore_ascii_case("horizontal") => Ok(Some("horizontal")),
        Some(v) if v.eq_ignore_ascii_case("vertical") => Ok(Some("vertical")),
        Some(v) => anyhow::bail!("Invalid flip '{}': expected horizontal|vertical", v),
    }
}

fn parse_overlay_position(
    s: Option<&str>,
    default: OverlayPosition,
) -> anyhow::Result<OverlayPosition> {
    let Some(v) = s else {
        return Ok(default);
    };
    let v = v.to_ascii_lowercase();
    match v.as_str() {
        "top" => Ok(OverlayPosition::Top),
        "bottom" => Ok(OverlayPosition::Bottom),
        "top-left" | "top_left" => Ok(OverlayPosition::TopLeft),
        "top-right" | "top_right" => Ok(OverlayPosition::TopRight),
        "bottom-left" | "bottom_left" => Ok(OverlayPosition::BottomLeft),
        "bottom-right" | "bottom_right" => Ok(OverlayPosition::BottomRight),
        "center" => Ok(OverlayPosition::Center),
        _ => anyhow::bail!("Invalid overlay position '{}'", v),
    }
}

fn parse_ec_level(s: Option<&str>) -> anyhow::Result<EcLevel> {
    match s.unwrap_or("m").to_ascii_lowercase().as_str() {
        "l" => Ok(EcLevel::L),
        "m" => Ok(EcLevel::M),
        "q" => Ok(EcLevel::Q),
        "h" => Ok(EcLevel::H),
        other => anyhow::bail!("Invalid QR ec level '{}'", other),
    }
}

fn load_pdk(req: &GenerateRequest) -> anyhow::Result<PdkConfig> {
    if let Some(toml) = req.custom_pdk_toml.as_deref()
        && !toml.trim().is_empty()
    {
        return PdkConfig::from_toml_str(toml);
    }
    let name = req.pdk_name.as_deref().unwrap_or("sky130");
    PdkConfig::builtin(name)
}

fn apply_overlays(bitmap: &mut ArtworkBitmap, req: &GenerateRequest) -> anyhow::Result<()> {
    let margin = req.overlay_margin.unwrap_or(2);
    if let Some(text) = req.text.as_deref() {
        let pos = parse_overlay_position(req.text_position.as_deref(), OverlayPosition::Bottom)?;
        let overlay = render_text(text, req.text_scale.unwrap_or(1), 0, 2);
        let (x, y) = pos.place(
            bitmap.width,
            bitmap.height,
            overlay.width,
            overlay.height,
            margin,
        );
        bitmap.composite(&overlay, x, y);
    }
    if let Some(data) = req.qr.as_deref() {
        let pos = parse_overlay_position(req.qr_position.as_deref(), OverlayPosition::BottomRight)?;
        let ec = parse_ec_level(req.qr_ec_level.as_deref())?;
        let overlay = render_qr(data, req.qr_module_size.unwrap_or(2), ec, 4)?;
        let (x, y) = pos.place(
            bitmap.width,
            bitmap.height,
            overlay.width,
            overlay.height,
            margin,
        );
        bitmap.composite(&overlay, x, y);
    }
    Ok(())
}

pub(crate) fn run_pipeline(req: &GenerateRequest) -> anyhow::Result<PipelineOutput> {
    let pdk = load_pdk(req)?;
    let strategy = parse_strategy(req.strategy.as_deref())?;
    let placement = if req.separated.unwrap_or(false) {
        PixelPlacement::Separated
    } else {
        PixelPlacement::Touching
    };
    let dither = if req.dither.unwrap_or(false) {
        DitherMode::FloydSteinberg
    } else {
        DitherMode::Off
    };
    let threshold = parse_threshold(req.threshold.as_deref())?;

    let mut bitmap = bitmap_from_rgba(&req.pixels, req.width, req.height, threshold, dither)?;

    if req.invert.unwrap_or(false) {
        bitmap.invert();
    }
    if let Some(rot) = req.rotate
        && rot != 0
    {
        anyhow::ensure!(
            matches!(rot, 0 | 90 | 180 | 270),
            "rotate must be one of 0/90/180/270"
        );
        bitmap.rotate(rot);
    }
    match parse_flip(req.flip.as_deref())? {
        Some("horizontal") => bitmap.flip_horizontal(),
        Some("vertical") => bitmap.flip_vertical(),
        _ => {}
    }

    apply_overlays(&mut bitmap, req)?;

    let profile = &pdk.layer_profiles()[0];
    let density_enforce = !req.no_density_enforce.unwrap_or(false);
    let force = req.force.unwrap_or(false);
    let no_check_drc = req.no_check_drc.unwrap_or(false);
    let bitmap_density = bitmap.density();

    let mut working = bitmap.clone();
    let rects = generate_layer_polygons(
        &mut working,
        &pdk,
        &profile.drc,
        strategy,
        placement,
        density_enforce,
        force,
    )?;
    let violations = if no_check_drc {
        Vec::new()
    } else {
        check_drc(&rects, pdk.pdk.db_units_per_um, &profile.drc)
    };

    let bb = bounding_box(&rects).unwrap_or(Rect::new(0, 0, 0, 0));
    let dbu_per_um = pdk.pdk.db_units_per_um as f64;
    let stats = StatsResponse {
        polygon_count: rects.len(),
        width_dbu: bb.width().0,
        height_dbu: bb.height().0,
        width_um: bb.width().0 as f64 / dbu_per_um,
        height_um: bb.height().0 as f64 / dbu_per_um,
        bitmap_width: bitmap.width,
        bitmap_height: bitmap.height,
        bitmap_density,
    };

    Ok(PipelineOutput {
        pdk,
        rects,
        violations,
        stats,
    })
}

pub(crate) fn generate_from_pixels(req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
    let out = run_pipeline(&req)?;
    Ok(GenerateResponse {
        rects: out
            .rects
            .iter()
            .map(|r| [r.x0.0, r.y0.0, r.x1.0, r.y1.0])
            .collect(),
        violations: out
            .violations
            .iter()
            .map(|v| ViolationResponse {
                rule: v.rule.to_string(),
                rect_index: v.rect_index,
                other_index: v.other_index,
                value: v.value,
                limit: v.limit,
                location: [v.location.x.0, v.location.y.0],
            })
            .collect(),
        stats: out.stats,
    })
}

pub(crate) fn generate_gds_bytes(req: GenerateRequest) -> anyhow::Result<Vec<u8>> {
    let out = run_pipeline(&req)?;
    let profile = &out.pdk.layer_profiles()[0];
    let cell_name = req.cell_name.as_deref().unwrap_or("artwork");
    let library_name = req.library_name.as_deref().unwrap_or("fabbula");
    write_gds_multi_to_bytes(
        &[LayerRects {
            rects: &out.rects,
            layer: profile.gds_layer,
            datatype: profile.gds_datatype,
        }],
        cell_name,
        library_name,
        out.pdk.pdk.db_units_per_um,
    )
}

pub(crate) fn validate_pdk_toml(toml_content: &str) -> PdkValidationResponse {
    match PdkConfig::from_toml_str(toml_content) {
        Ok(pdk) => PdkValidationResponse {
            valid: true,
            error: None,
            info: Some(PdkValidationInfo {
                name: pdk.pdk.name.clone(),
                description: pdk.pdk.description.clone(),
                node_nm: pdk.pdk.node_nm,
                db_units_per_um: pdk.pdk.db_units_per_um,
                pixel_pitch_um: pdk.pixel_pitch_um(),
                layers: pdk.layer_profiles().len(),
            }),
        },
        Err(e) => PdkValidationResponse {
            valid: false,
            error: Some(e.to_string()),
            info: None,
        },
    }
}

pub(crate) fn list_builtin_pdks() -> anyhow::Result<Vec<BuiltinPdkInfo>> {
    let mut out = Vec::new();
    for builtin in PdkConfig::list_builtins() {
        let pdk = PdkConfig::builtin(builtin.name())?;
        out.push(BuiltinPdkInfo {
            name: builtin.name().to_string(),
            description: pdk.pdk.description.clone(),
            node_nm: pdk.pdk.node_nm,
            min_width: pdk.drc.min_width,
            min_spacing: pdk.drc.min_spacing,
        });
    }
    Ok(out)
}
