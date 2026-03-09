// Copyright 2026 Daniel Keller <daniel.keller.m@gmail.com>
// Licensed under the Apache License, Version 2.0.
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};
use eframe::egui::{self, Color32, CursorIcon, Pos2, Sense, Stroke, TextureHandle, Vec2};
use fabbula::OverlayPosition;
use fabbula::artwork::{ArtworkBitmap, DitherMode, ThresholdMode, load_artwork};
use fabbula::drc::check_drc;
use fabbula::gdsio::{LayerRects, merge_into_gds_multi, read_gds_bounds, write_gds_multi};
use fabbula::generation::generate_layer_polygons;
use fabbula::lef::{LefLayer, write_lef_multi};
use fabbula::pdk::{BuiltinPdk, PdkConfig};
use fabbula::polygon::{PixelPlacement, PolygonStrategy, Rect as PolyRect, bounding_box};
use fabbula::preview::DEFAULT_LAYER_COLORS;
use fabbula::preview::{HtmlLayer, SvgLayer, write_html_preview_multi, write_svg_multi};
use fabbula::qr::{EcLevel, render_qr};
use fabbula::text::{TextFont, render_text_with_font};
use rfd::FileDialog;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(feature = "rayon")]
use rayon::prelude::*;

const BUILTIN_PDKS: [BuiltinPdk; 6] = [
    BuiltinPdk::Sky130,
    BuiltinPdk::IhpSg13g2,
    BuiltinPdk::Gf180mcu,
    BuiltinPdk::FreePdk45,
    BuiltinPdk::Asap7,
    BuiltinPdk::Fabbula2,
];
const PREVIEW_MAX_DIM: u32 = 768;
const AUTO_REGEN_DEBOUNCE_MS: u64 = 250;
const DEFAULT_CORE_INSET_DIV: i32 = 10;
const INTERACTION_REGEN_DEBOUNCE_MS: u64 = 120;
const INTERACTION_PREVIEW_MAX_DIM: u32 = 512;
const DEFAULT_CANVAS_SIZE: u32 = 1024;

#[derive(Debug, Clone)]
struct GenerationConfig {
    use_custom_pdk: bool,
    selected_builtin_pdk: BuiltinPdk,
    custom_pdk_toml: String,
    strategy: Strategy,
    separated: bool,
    invert: bool,
    rotate: u32,
    flip: Flip,
    text_overlay: String,
    text_position: OverlayPosition,
    text_scale: u32,
    text_font: TextFont,
    text_manual_xy: Option<(u32, u32)>,
    qr_overlay: String,
    qr_position: OverlayPosition,
    qr_module_size: u32,
    qr_ec_level: EcLevel,
    qr_manual_xy: Option<(u32, u32)>,
    overlay_margin: u32,
    overlay_knockout_padding: u32,
    no_check_drc: bool,
    no_density_enforce: bool,
    force: bool,
    canvas_width: u32,
    canvas_height: u32,
    image_x: u32,
    image_y: u32,
    image_scale_x_pct: u32,
    image_scale_y_pct: u32,
    use_die_bounds: bool,
    die_bounds_dbu: Option<PolyRect>,
}

#[derive(Debug, Clone, Copy)]
enum JobKind {
    Preview,
    Full,
}

#[derive(Debug, Clone)]
struct GenerationJob {
    epoch: u64,
    kind: JobKind,
    bitmap: ArtworkBitmap,
    cfg: GenerationConfig,
}

#[derive(Debug)]
enum WorkerMessage {
    Done {
        epoch: u64,
        kind: JobKind,
        out: Box<JobUiOutput>,
    },
    Failed {
        epoch: u64,
        kind: JobKind,
        error: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AutoGenState {
    input_path: String,
    use_custom_pdk: bool,
    selected_builtin_pdk: BuiltinPdk,
    custom_pdk_toml: String,
    threshold: String,
    strategy: Strategy,
    separated: bool,
    invert: bool,
    dither: bool,
    rotate: u32,
    flip: Flip,
    no_check_drc: bool,
    no_density_enforce: bool,
    force: bool,
    text_overlay: String,
    text_position: OverlayPosition,
    text_scale: u32,
    text_font: TextFont,
    text_manual_xy: Option<(u32, u32)>,
    qr_overlay: String,
    qr_position: OverlayPosition,
    qr_module_size: u32,
    qr_ec_level: EcLevel,
    qr_manual_xy: Option<(u32, u32)>,
    overlay_margin: u32,
    overlay_knockout_padding: u32,
    canvas_width: u32,
    canvas_height: u32,
    image_x: u32,
    image_y: u32,
    image_scale_x_pct: u32,
    image_scale_y_pct: u32,
    use_die_bounds: bool,
    die_bounds_dbu: Option<PolyRect>,
    preview_max_dim: u32,
    auto_full_res: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Strategy {
    PixelRects,
    RowMerge,
    GreedyMerge,
    HistogramMerge,
}

impl Strategy {
    fn as_str(self) -> &'static str {
        match self {
            Self::PixelRects => "pixel-rects",
            Self::RowMerge => "row-merge",
            Self::GreedyMerge => "greedy-merge",
            Self::HistogramMerge => "histogram-merge",
        }
    }

    fn to_polygon(self) -> PolygonStrategy {
        match self {
            Self::PixelRects => PolygonStrategy::PixelRects,
            Self::RowMerge => PolygonStrategy::RowMerge,
            Self::GreedyMerge => PolygonStrategy::GreedyMerge,
            Self::HistogramMerge => PolygonStrategy::HistogramMerge,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flip {
    None,
    Horizontal,
    Vertical,
}

impl Flip {
    fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }
}

#[derive(Debug, Clone)]
struct GuiResult {
    rects: Vec<PolyRect>,
    pdk: PdkConfig,
    layer_name: String,
    layer: i16,
    datatype: i16,
    full_bb: PolyRect,
    bitmap_w: u32,
    bitmap_h: u32,
    pitch_dbu: i32,
    pixel_w_dbu: i32,
    source_w_px: u32,
    source_h_px: u32,
}

#[derive(Debug, Clone)]
struct JobUiOutput {
    result: GuiResult,
    stats: String,
    drc_summary: String,
    elapsed_ms: f64,
    rect_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragOverlay {
    ImageMove,
    ImageResize(ResizeCorner),
    Text,
    TextResize(ResizeCorner),
    Qr,
    QrResize(ResizeCorner),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResizeCorner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Copy)]
struct ImageResizeDrag {
    corner: ResizeCorner,
    anchor_px: (i32, i32),
    start_image_w: u32,
    start_image_h: u32,
    start_scale_x_pct: u32,
    start_scale_y_pct: u32,
}

#[derive(Debug, Clone, Copy)]
struct UniformResizeDrag {
    corner: ResizeCorner,
    anchor_px: (i32, i32),
    start_w: u32,
    start_h: u32,
    start_value: u32,
}

#[derive(Debug, Clone)]
struct RectSpatialIndex {
    cell_dbu: i32,
    map: HashMap<(i32, i32), Vec<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct GuiSettings {
    theme_dark: bool,
    selected_builtin_pdk: String,
    use_custom_pdk: bool,
    custom_pdk_toml: String,
    threshold: String,
    strategy: String,
    separated: bool,
    invert: bool,
    dither: bool,
    rotate: u32,
    flip: String,
    no_check_drc: bool,
    no_density_enforce: bool,
    force: bool,
    text_overlay: String,
    text_position: String,
    text_scale: u32,
    text_font: String,
    qr_overlay: String,
    qr_position: String,
    qr_module_size: u32,
    qr_ec_level: String,
    overlay_margin: u32,
    #[serde(default)]
    overlay_knockout_padding: u32,
    input_path: String,
    output_path: String,
    cell_name: String,
    library_name: String,
    canvas_width: u32,
    canvas_height: u32,
    image_x: u32,
    image_y: u32,
    #[serde(default = "default_image_scale_pct")]
    image_scale_pct: u32,
    #[serde(default)]
    image_scale_x_pct: Option<u32>,
    #[serde(default)]
    image_scale_y_pct: Option<u32>,
    #[serde(default = "default_lock_aspect_ratio")]
    lock_aspect_ratio: bool,
    chip_gds_path: String,
    chip_cell_name: String,
    use_die_bounds: bool,
    preview_max_dim: u32,
    auto_full_res: bool,
}

#[derive(Debug, Clone)]
struct PdkValidationUi {
    valid: bool,
    message: String,
}

pub fn run_gui(initial_input: Option<PathBuf>, initial_pdk: Option<String>) -> Result<()> {
    let options = eframe::NativeOptions::default();
    let input_path = initial_input
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "fabbula.png".to_string());
    let builtin = initial_pdk
        .as_deref()
        .and_then(|s| s.parse::<BuiltinPdk>().ok())
        .unwrap_or(BuiltinPdk::Sky130);
    let mut app = FabbulaGuiApp::new(input_path.clone(), builtin);
    if let Some(settings) = load_gui_settings() {
        app.apply_settings(settings);
    }
    if !input_path.is_empty() {
        app.input_path = input_path;
    }
    if let Some(pdk_name) = initial_pdk
        && let Ok(pdk) = pdk_name.parse::<BuiltinPdk>()
    {
        app.selected_builtin_pdk = pdk;
    }
    eframe::run_native("fabbula gui", options, Box::new(move |_| Ok(Box::new(app))))
        .map_err(|e| anyhow::anyhow!("{e}"))
}

struct FabbulaGuiApp {
    input_path: String,
    output_path: String,
    cell_name: String,
    library_name: String,

    selected_builtin_pdk: BuiltinPdk,
    use_custom_pdk: bool,
    template_builtin_pdk: BuiltinPdk,
    custom_pdk_toml: String,
    pdk_validation: PdkValidationUi,

    threshold: String,
    strategy: Strategy,
    separated: bool,
    invert: bool,
    dither: bool,
    rotate: u32,
    flip: Flip,
    no_check_drc: bool,
    no_density_enforce: bool,
    force: bool,

    text_overlay: String,
    text_position: OverlayPosition,
    text_scale: u32,
    text_font: TextFont,
    text_manual_xy: Option<(u32, u32)>,
    qr_overlay: String,
    qr_position: OverlayPosition,
    qr_module_size: u32,
    qr_ec_level: EcLevel,
    qr_manual_xy: Option<(u32, u32)>,
    overlay_margin: u32,
    overlay_knockout_padding: u32,
    canvas_width: u32,
    canvas_height: u32,
    image_x: u32,
    image_y: u32,
    image_scale_x_pct: u32,
    image_scale_y_pct: u32,
    lock_aspect_ratio: bool,
    chip_gds_path: String,
    chip_cell_name: String,
    use_die_bounds: bool,
    die_bounds_dbu: Option<PolyRect>,

    status: String,
    status_is_error: bool,
    stats: String,
    drc_summary: String,
    preview_zoom: f32,
    preview_pan: Vec2,
    theme_dark: bool,
    selected_rect: Option<usize>,
    hovered_rect: Option<usize>,
    dragging_overlay: Option<DragOverlay>,
    panning_viewport: bool,
    drag_offset_px: (i32, i32),
    image_resize_drag: Option<ImageResizeDrag>,
    text_resize_drag: Option<UniformResizeDrag>,
    qr_resize_drag: Option<UniformResizeDrag>,
    focus_rect: Option<PolyRect>,
    cached_source_key: Option<(String, String, bool)>,
    cached_source_bitmap: Option<ArtworkBitmap>,
    cached_interaction_texture_key: Option<(String, String, bool, u32, Flip, bool)>,
    cached_interaction_texture: Option<TextureHandle>,
    cached_text_texture_key: Option<(String, u32, TextFont)>,
    cached_text_texture: Option<TextureHandle>,
    cached_qr_texture_key: Option<(String, u32, EcLevel)>,
    cached_qr_texture: Option<TextureHandle>,
    cached_poly_texture_key: Option<(u64, bool, usize)>,
    cached_poly_texture: Option<TextureHandle>,
    rect_index: Option<RectSpatialIndex>,
    next_epoch: u64,
    latest_epoch: u64,
    preview_job_tx: Sender<GenerationJob>,
    full_job_tx: Sender<GenerationJob>,
    worker_rx: Receiver<WorkerMessage>,
    latest_epoch_atomic: Arc<AtomicU64>,
    pending_preview_epoch: Option<u64>,
    pending_full_epoch: Option<u64>,
    result_is_preview: bool,
    preview_max_dim: u32,
    auto_full_res: bool,
    preview_ms: Option<f64>,
    fullres_ms: Option<f64>,
    rect_count_trend: VecDeque<usize>,
    last_saved_settings: Option<GuiSettings>,
    last_autogen_state: Option<AutoGenState>,
    auto_regen_deadline: Option<Instant>,
    interaction_active: bool,
    show_live_raster_overlay: bool,
    last_result: Option<GuiResult>,
}

impl FabbulaGuiApp {
    fn new(input_path: String, selected_builtin_pdk: BuiltinPdk) -> Self {
        let template = selected_builtin_pdk;
        let custom_pdk_toml = template.toml_content().to_string();
        let (preview_job_tx, preview_job_rx) = mpsc::channel::<GenerationJob>();
        let (full_job_tx, full_job_rx) = mpsc::channel::<GenerationJob>();
        let (worker_tx, worker_rx) = mpsc::channel::<WorkerMessage>();
        let latest_epoch_atomic = Arc::new(AtomicU64::new(0));
        spawn_generation_worker(
            preview_job_rx,
            worker_tx.clone(),
            true,
            Arc::clone(&latest_epoch_atomic),
        );
        spawn_generation_worker(
            full_job_rx,
            worker_tx,
            false,
            Arc::clone(&latest_epoch_atomic),
        );
        Self {
            input_path,
            output_path: "artwork.gds".to_string(),
            cell_name: "artwork".to_string(),
            library_name: "fabbula".to_string(),
            selected_builtin_pdk,
            use_custom_pdk: false,
            template_builtin_pdk: template,
            custom_pdk_toml,
            pdk_validation: PdkValidationUi {
                valid: true,
                message: "Valid custom PDK.".to_string(),
            },
            threshold: "128".to_string(),
            strategy: Strategy::GreedyMerge,
            separated: false,
            invert: false,
            dither: false,
            rotate: 0,
            flip: Flip::None,
            no_check_drc: false,
            no_density_enforce: false,
            force: false,
            text_overlay: String::new(),
            text_position: OverlayPosition::Bottom,
            text_scale: 1,
            text_font: TextFont::Mono,
            text_manual_xy: None,
            qr_overlay: String::new(),
            qr_position: OverlayPosition::BottomRight,
            qr_module_size: 2,
            qr_ec_level: EcLevel::M,
            qr_manual_xy: None,
            overlay_margin: 2,
            overlay_knockout_padding: 2,
            canvas_width: DEFAULT_CANVAS_SIZE,
            canvas_height: DEFAULT_CANVAS_SIZE,
            image_x: 0,
            image_y: 0,
            image_scale_x_pct: 100,
            image_scale_y_pct: 100,
            lock_aspect_ratio: true,
            chip_gds_path: String::new(),
            chip_cell_name: String::new(),
            use_die_bounds: false,
            die_bounds_dbu: None,
            status: "Set image path, adjust options, then click Generate.".to_string(),
            status_is_error: false,
            stats: "No generated output yet.".to_string(),
            drc_summary: "DRC: n/a".to_string(),
            preview_zoom: 1.0,
            preview_pan: Vec2::ZERO,
            theme_dark: false,
            selected_rect: None,
            hovered_rect: None,
            dragging_overlay: None,
            panning_viewport: false,
            drag_offset_px: (0, 0),
            image_resize_drag: None,
            text_resize_drag: None,
            qr_resize_drag: None,
            focus_rect: None,
            cached_source_key: None,
            cached_source_bitmap: None,
            cached_interaction_texture_key: None,
            cached_interaction_texture: None,
            cached_text_texture_key: None,
            cached_text_texture: None,
            cached_qr_texture_key: None,
            cached_qr_texture: None,
            cached_poly_texture_key: None,
            cached_poly_texture: None,
            rect_index: None,
            next_epoch: 0,
            latest_epoch: 0,
            preview_job_tx,
            full_job_tx,
            worker_rx,
            latest_epoch_atomic,
            pending_preview_epoch: None,
            pending_full_epoch: None,
            result_is_preview: false,
            preview_max_dim: PREVIEW_MAX_DIM,
            auto_full_res: true,
            preview_ms: None,
            fullres_ms: None,
            rect_count_trend: VecDeque::with_capacity(32),
            last_saved_settings: None,
            last_autogen_state: None,
            auto_regen_deadline: None,
            interaction_active: false,
            show_live_raster_overlay: false,
            last_result: None,
        }
    }

    fn parse_threshold(&self) -> Result<ThresholdMode> {
        if self.threshold.eq_ignore_ascii_case("otsu") {
            Ok(ThresholdMode::Otsu)
        } else if self.threshold.eq_ignore_ascii_case("auto") {
            Ok(ThresholdMode::Auto)
        } else if self.threshold.eq_ignore_ascii_case("alpha") {
            Ok(ThresholdMode::Alpha(128))
        } else if let Ok(v) = self.threshold.parse::<u8>() {
            Ok(ThresholdMode::Luminance(v))
        } else {
            anyhow::bail!("Invalid threshold: use 0-255, otsu, auto, or alpha")
        }
    }

    fn refresh_pdk_validation(&mut self) {
        match PdkConfig::from_toml_str(&self.custom_pdk_toml) {
            Ok(cfg) => {
                self.pdk_validation = PdkValidationUi {
                    valid: true,
                    message: format!(
                        "Valid: {} | node={}nm | pitch={:.4}um | layers={}",
                        cfg.pdk.name,
                        cfg.pdk.node_nm,
                        cfg.pixel_pitch_um(),
                        cfg.layer_profiles().len()
                    ),
                };
            }
            Err(e) => {
                self.pdk_validation = PdkValidationUi {
                    valid: false,
                    message: format!("Invalid custom PDK: {e}"),
                };
            }
        }
    }

    fn apply_settings(&mut self, s: GuiSettings) {
        self.theme_dark = s.theme_dark;
        if let Ok(pdk) = s.selected_builtin_pdk.parse::<BuiltinPdk>() {
            self.selected_builtin_pdk = pdk;
        }
        self.use_custom_pdk = s.use_custom_pdk;
        self.custom_pdk_toml = s.custom_pdk_toml;
        self.threshold = s.threshold;
        self.strategy = match s.strategy.as_str() {
            "pixel-rects" => Strategy::PixelRects,
            "row-merge" => Strategy::RowMerge,
            "histogram-merge" => Strategy::HistogramMerge,
            _ => Strategy::GreedyMerge,
        };
        self.separated = s.separated;
        self.invert = s.invert;
        self.dither = s.dither;
        self.rotate = s.rotate;
        self.flip = match s.flip.as_str() {
            "horizontal" => Flip::Horizontal,
            "vertical" => Flip::Vertical,
            _ => Flip::None,
        };
        self.no_check_drc = s.no_check_drc;
        self.no_density_enforce = s.no_density_enforce;
        self.force = s.force;
        self.text_overlay = s.text_overlay;
        self.text_position =
            parse_overlay_position(&s.text_position).unwrap_or(OverlayPosition::Bottom);
        self.text_scale = s.text_scale.max(1);
        self.text_font = parse_text_font(&s.text_font).unwrap_or(TextFont::Mono);
        self.qr_overlay = s.qr_overlay;
        self.qr_position =
            parse_overlay_position(&s.qr_position).unwrap_or(OverlayPosition::BottomRight);
        self.qr_module_size = s.qr_module_size.max(1);
        self.qr_ec_level = parse_ec_level(&s.qr_ec_level).unwrap_or(EcLevel::M);
        self.overlay_margin = s.overlay_margin;
        self.overlay_knockout_padding = s.overlay_knockout_padding;
        self.input_path = s.input_path;
        self.output_path = s.output_path;
        self.cell_name = s.cell_name;
        self.library_name = s.library_name;
        if s.canvas_width == 0 && s.canvas_height == 0 {
            self.canvas_width = DEFAULT_CANVAS_SIZE;
            self.canvas_height = DEFAULT_CANVAS_SIZE;
        } else {
            self.canvas_width = s.canvas_width;
            self.canvas_height = s.canvas_height;
        }
        self.image_x = s.image_x;
        self.image_y = s.image_y;
        let base_scale = s.image_scale_pct.max(1);
        self.image_scale_x_pct = s.image_scale_x_pct.unwrap_or(base_scale).max(1);
        self.image_scale_y_pct = s.image_scale_y_pct.unwrap_or(base_scale).max(1);
        self.lock_aspect_ratio = s.lock_aspect_ratio;
        self.chip_gds_path = s.chip_gds_path;
        self.chip_cell_name = s.chip_cell_name;
        self.use_die_bounds = s.use_die_bounds;
        self.die_bounds_dbu = None;
        self.preview_max_dim = s.preview_max_dim.max(128);
        self.auto_full_res = s.auto_full_res;
        self.refresh_pdk_validation();
    }

    fn snapshot_settings(&self) -> GuiSettings {
        GuiSettings {
            theme_dark: self.theme_dark,
            selected_builtin_pdk: self.selected_builtin_pdk.name().to_string(),
            use_custom_pdk: self.use_custom_pdk,
            custom_pdk_toml: self.custom_pdk_toml.clone(),
            threshold: self.threshold.clone(),
            strategy: self.strategy.as_str().to_string(),
            separated: self.separated,
            invert: self.invert,
            dither: self.dither,
            rotate: self.rotate,
            flip: self.flip.as_str().to_string(),
            no_check_drc: self.no_check_drc,
            no_density_enforce: self.no_density_enforce,
            force: self.force,
            text_overlay: self.text_overlay.clone(),
            text_position: overlay_position_str(self.text_position).to_string(),
            text_scale: self.text_scale,
            text_font: text_font_str(self.text_font).to_string(),
            qr_overlay: self.qr_overlay.clone(),
            qr_position: overlay_position_str(self.qr_position).to_string(),
            qr_module_size: self.qr_module_size,
            qr_ec_level: ec_level_str(self.qr_ec_level).to_string(),
            overlay_margin: self.overlay_margin,
            overlay_knockout_padding: self.overlay_knockout_padding,
            input_path: self.input_path.clone(),
            output_path: self.output_path.clone(),
            cell_name: self.cell_name.clone(),
            library_name: self.library_name.clone(),
            canvas_width: self.canvas_width,
            canvas_height: self.canvas_height,
            image_x: self.image_x,
            image_y: self.image_y,
            image_scale_pct: self.image_scale_x_pct,
            image_scale_x_pct: Some(self.image_scale_x_pct),
            image_scale_y_pct: Some(self.image_scale_y_pct),
            lock_aspect_ratio: self.lock_aspect_ratio,
            chip_gds_path: self.chip_gds_path.clone(),
            chip_cell_name: self.chip_cell_name.clone(),
            use_die_bounds: self.use_die_bounds,
            preview_max_dim: self.preview_max_dim,
            auto_full_res: self.auto_full_res,
        }
    }

    fn generation_config(&self) -> GenerationConfig {
        GenerationConfig {
            use_custom_pdk: self.use_custom_pdk,
            selected_builtin_pdk: self.selected_builtin_pdk,
            custom_pdk_toml: self.custom_pdk_toml.clone(),
            strategy: self.strategy,
            separated: self.separated,
            invert: self.invert,
            rotate: self.rotate,
            flip: self.flip,
            text_overlay: self.text_overlay.clone(),
            text_position: self.text_position,
            text_scale: self.text_scale,
            text_font: self.text_font,
            text_manual_xy: self.text_manual_xy,
            qr_overlay: self.qr_overlay.clone(),
            qr_position: self.qr_position,
            qr_module_size: self.qr_module_size,
            qr_ec_level: self.qr_ec_level,
            qr_manual_xy: self.qr_manual_xy,
            overlay_margin: self.overlay_margin,
            overlay_knockout_padding: self.overlay_knockout_padding,
            no_check_drc: self.no_check_drc,
            no_density_enforce: self.no_density_enforce,
            force: self.force,
            canvas_width: self.canvas_width,
            canvas_height: self.canvas_height,
            image_x: self.image_x,
            image_y: self.image_y,
            image_scale_x_pct: self.image_scale_x_pct,
            image_scale_y_pct: self.image_scale_y_pct,
            use_die_bounds: self.use_die_bounds,
            die_bounds_dbu: self.die_bounds_dbu,
        }
    }

    fn autogen_state(&self) -> AutoGenState {
        AutoGenState {
            input_path: self.input_path.clone(),
            use_custom_pdk: self.use_custom_pdk,
            selected_builtin_pdk: self.selected_builtin_pdk,
            custom_pdk_toml: self.custom_pdk_toml.clone(),
            threshold: self.threshold.clone(),
            strategy: self.strategy,
            separated: self.separated,
            invert: self.invert,
            dither: self.dither,
            rotate: self.rotate,
            flip: self.flip,
            no_check_drc: self.no_check_drc,
            no_density_enforce: self.no_density_enforce,
            force: self.force,
            text_overlay: self.text_overlay.clone(),
            text_position: self.text_position,
            text_scale: self.text_scale,
            text_font: self.text_font,
            text_manual_xy: self.text_manual_xy,
            qr_overlay: self.qr_overlay.clone(),
            qr_position: self.qr_position,
            qr_module_size: self.qr_module_size,
            qr_ec_level: self.qr_ec_level,
            qr_manual_xy: self.qr_manual_xy,
            overlay_margin: self.overlay_margin,
            overlay_knockout_padding: self.overlay_knockout_padding,
            canvas_width: self.canvas_width,
            canvas_height: self.canvas_height,
            image_x: self.image_x,
            image_y: self.image_y,
            image_scale_x_pct: self.image_scale_x_pct,
            image_scale_y_pct: self.image_scale_y_pct,
            use_die_bounds: self.use_die_bounds,
            die_bounds_dbu: self.die_bounds_dbu,
            preview_max_dim: self.preview_max_dim,
            auto_full_res: self.auto_full_res,
        }
    }

    fn load_source_bitmap_cached(&mut self) -> Result<ArtworkBitmap> {
        let key = (self.input_path.clone(), self.threshold.clone(), self.dither);
        if let (Some(existing_key), Some(existing_bitmap)) =
            (&self.cached_source_key, &self.cached_source_bitmap)
            && existing_key == &key
        {
            return Ok(existing_bitmap.clone());
        }

        let threshold = self.parse_threshold()?;
        let dither_mode = if self.dither {
            DitherMode::FloydSteinberg
        } else {
            DitherMode::Off
        };
        let bitmap = load_artwork(Path::new(&self.input_path), threshold, None, dither_mode)
            .with_context(|| format!("Failed to load image '{}'", self.input_path))?;
        self.cached_source_key = Some(key);
        self.cached_source_bitmap = Some(bitmap.clone());
        Ok(bitmap)
    }

    fn interaction_texture(&mut self, ctx: &egui::Context) -> Result<Option<TextureHandle>> {
        let key = (
            self.input_path.clone(),
            self.threshold.clone(),
            self.dither,
            self.rotate,
            self.flip,
            self.invert,
        );
        if let (Some(existing_key), Some(existing_texture)) = (
            &self.cached_interaction_texture_key,
            &self.cached_interaction_texture,
        ) && existing_key == &key
        {
            return Ok(Some(existing_texture.clone()));
        }
        let mut bitmap = self.load_source_bitmap_cached()?;
        apply_bitmap_transforms(&mut bitmap, &self.generation_config())?;
        let image = bitmap_to_color_image(&bitmap);
        let texture = ctx.load_texture("interaction-source", image, egui::TextureOptions::NEAREST);
        self.cached_interaction_texture_key = Some(key);
        self.cached_interaction_texture = Some(texture.clone());
        Ok(Some(texture))
    }

    fn interaction_text_texture(&mut self, ctx: &egui::Context) -> Result<Option<TextureHandle>> {
        if self.text_overlay.trim().is_empty() {
            return Ok(None);
        }
        let key = (
            self.text_overlay.clone(),
            self.text_scale.max(1),
            self.text_font,
        );
        if let (Some(existing_key), Some(existing_texture)) =
            (&self.cached_text_texture_key, &self.cached_text_texture)
            && existing_key == &key
        {
            return Ok(Some(existing_texture.clone()));
        }
        let bitmap = render_text_with_font(
            &self.text_overlay,
            self.text_scale.max(1),
            0,
            2,
            self.text_font,
        );
        let image = bitmap_to_color_image(&bitmap);
        let texture = ctx.load_texture("interaction-text", image, egui::TextureOptions::NEAREST);
        self.cached_text_texture_key = Some(key);
        self.cached_text_texture = Some(texture.clone());
        Ok(Some(texture))
    }

    fn interaction_qr_texture(&mut self, ctx: &egui::Context) -> Result<Option<TextureHandle>> {
        if self.qr_overlay.trim().is_empty() {
            return Ok(None);
        }
        let key = (
            self.qr_overlay.clone(),
            self.qr_module_size.max(1),
            self.qr_ec_level,
        );
        if let (Some(existing_key), Some(existing_texture)) =
            (&self.cached_qr_texture_key, &self.cached_qr_texture)
            && existing_key == &key
        {
            return Ok(Some(existing_texture.clone()));
        }
        let bitmap = render_qr(
            &self.qr_overlay,
            self.qr_module_size.max(1),
            self.qr_ec_level,
            4,
        )?;
        let image = bitmap_to_color_image(&bitmap);
        let texture = ctx.load_texture("interaction-qr", image, egui::TextureOptions::NEAREST);
        self.cached_qr_texture_key = Some(key);
        self.cached_qr_texture = Some(texture.clone());
        Ok(Some(texture))
    }

    fn polygon_layer_texture(&mut self, ctx: &egui::Context) -> Option<TextureHandle> {
        let result = self.last_result.as_ref()?;
        let key = (
            self.latest_epoch,
            self.result_is_preview,
            result.rects.len(),
        );
        if let (Some(existing_key), Some(existing_texture)) =
            (&self.cached_poly_texture_key, &self.cached_poly_texture)
            && existing_key == &key
        {
            return Some(existing_texture.clone());
        }
        let img = rasterize_rects_to_image(result);
        let tex = ctx.load_texture("poly-layer", img, egui::TextureOptions::NEAREST);
        self.cached_poly_texture_key = Some(key);
        self.cached_poly_texture = Some(tex.clone());
        Some(tex)
    }

    fn transformed_source_dims(&mut self) -> Result<(u32, u32)> {
        let mut bitmap = self.load_source_bitmap_cached()?;
        if self.rotate != 0 {
            bitmap.rotate(self.rotate);
        }
        Ok((bitmap.width.max(1), bitmap.height.max(1)))
    }

    fn fit_image_to_die(&mut self) -> Result<()> {
        let die = self
            .die_bounds_dbu
            .context("Load die bounds from Chip GDS before fitting")?;
        let pdk = if self.use_custom_pdk {
            PdkConfig::from_toml_str(&self.custom_pdk_toml)?
        } else {
            PdkConfig::builtin(self.selected_builtin_pdk.name())?
        };
        let profile = &pdk.layer_profiles()[0];
        let min_w_um = pdk.snap_to_grid(profile.drc.min_width);
        let eff_s_um = pdk.snap_to_grid(profile.drc.effective_spacing());
        let touching = !self.separated;
        let pitch_um = if touching {
            min_w_um.max(eff_s_um)
        } else {
            min_w_um + eff_s_um
        };
        let pitch_dbu = pdk.um_to_dbu(pitch_um).0.max(1);
        let die_w_px = ((die.width().0 + pitch_dbu - 1) / pitch_dbu).max(1) as u32;
        let die_h_px = ((die.height().0 + pitch_dbu - 1) / pitch_dbu).max(1) as u32;
        let (src_w, src_h) = self.transformed_source_dims()?;
        let fit_scale = (die_w_px as f64 / src_w as f64).min(die_h_px as f64 / src_h as f64);
        let fit_pct = (fit_scale * 100.0).floor().max(1.0) as u32;
        let out_w = ((src_w as f64 * fit_pct as f64 / 100.0).round() as u32).max(1);
        let out_h = ((src_h as f64 * fit_pct as f64 / 100.0).round() as u32).max(1);
        self.use_die_bounds = true;
        self.canvas_width = die_w_px;
        self.canvas_height = die_h_px;
        self.image_scale_x_pct = fit_pct.max(1);
        self.image_scale_y_pct = fit_pct.max(1);
        self.image_x = die_w_px.saturating_sub(out_w) / 2;
        self.image_y = die_h_px.saturating_sub(out_h) / 2;
        Ok(())
    }

    fn generate_internal(&mut self, allow_full_res: bool) -> Result<()> {
        if self.input_path.trim().is_empty() {
            anyhow::bail!("Image path is empty");
        }
        if self.use_custom_pdk && !self.pdk_validation.valid {
            anyhow::bail!("Custom PDK is invalid; fix the TOML before generating");
        }

        let full_cfg = self.generation_config();
        let source = self.load_source_bitmap_cached()?;
        let preview_dim = if self.interaction_active {
            self.preview_max_dim.clamp(64, INTERACTION_PREVIEW_MAX_DIM)
        } else {
            self.preview_max_dim.max(64)
        };
        let preview_source = downsample_bitmap_nearest(&source, preview_dim);
        let mut preview_cfg = full_cfg.clone();
        if self.interaction_active {
            preview_cfg.strategy = Strategy::GreedyMerge;
        }
        if preview_source.width > 0 && source.width > 0 {
            let rx = preview_source.width as f64 / source.width as f64;
            if rx > 0.0 {
                preview_cfg.image_scale_x_pct = ((full_cfg.image_scale_x_pct as f64) / rx)
                    .round()
                    .clamp(1.0, u32::MAX as f64)
                    as u32;
            }
        }
        if preview_source.height > 0 && source.height > 0 {
            let ry = preview_source.height as f64 / source.height as f64;
            if ry > 0.0 {
                preview_cfg.image_scale_y_pct = ((full_cfg.image_scale_y_pct as f64) / ry)
                    .round()
                    .clamp(1.0, u32::MAX as f64)
                    as u32;
            }
        }
        self.next_epoch = self.next_epoch.wrapping_add(1);
        let epoch = self.next_epoch;
        self.latest_epoch = epoch;
        self.latest_epoch_atomic.store(epoch, Ordering::Relaxed);
        self.pending_preview_epoch = Some(epoch);
        self.pending_full_epoch = if allow_full_res && self.auto_full_res {
            Some(epoch)
        } else {
            None
        };
        self.status = if allow_full_res && self.auto_full_res {
            "Queued preview + full-resolution generation...".to_string()
        } else {
            "Queued preview generation...".to_string()
        };
        self.status_is_error = false;
        self.preview_zoom = self.preview_zoom.max(0.05);
        self.preview_job_tx
            .send(GenerationJob {
                epoch,
                kind: JobKind::Preview,
                bitmap: preview_source,
                cfg: preview_cfg,
            })
            .context("Preview worker unavailable")?;
        if allow_full_res && self.auto_full_res {
            self.full_job_tx
                .send(GenerationJob {
                    epoch,
                    kind: JobKind::Full,
                    bitmap: source,
                    cfg: full_cfg,
                })
                .context("Full worker unavailable")?;
            self.show_live_raster_overlay = true;
        } else {
            self.show_live_raster_overlay = false;
        }
        Ok(())
    }

    fn generate(&mut self, _ctx: &egui::Context) -> Result<()> {
        self.generate_internal(true)
    }

    fn poll_worker_results(&mut self) {
        while let Ok(msg) = self.worker_rx.try_recv() {
            match msg {
                WorkerMessage::Done { epoch, kind, out } => {
                    if epoch != self.latest_epoch {
                        continue;
                    }
                    let out = *out;
                    match kind {
                        JobKind::Preview => {
                            self.pending_preview_epoch = None;
                            self.preview_ms = Some(out.elapsed_ms);
                            push_rect_trend(&mut self.rect_count_trend, out.rect_count);
                            if self.pending_full_epoch == Some(epoch)
                                && self.show_live_raster_overlay
                            {
                                self.status =
                                    "Preview ready. Refining full resolution...".to_string();
                                self.status_is_error = false;
                                continue;
                            }
                            self.stats = out.stats;
                            self.drc_summary = out.drc_summary;
                            self.last_result = Some(out.result);
                            self.cached_poly_texture = None;
                            self.cached_poly_texture_key = None;
                            self.rect_index =
                                self.last_result.as_ref().map(build_rect_spatial_index);
                            self.result_is_preview = true;
                            self.selected_rect = None;
                            self.hovered_rect = None;
                            self.status = if self.pending_full_epoch == Some(epoch) {
                                "Preview ready. Refining full resolution...".to_string()
                            } else {
                                "Preview ready.".to_string()
                            };
                            self.status_is_error = false;
                        }
                        JobKind::Full => {
                            self.pending_full_epoch = None;
                            self.stats = out.stats;
                            self.drc_summary = out.drc_summary;
                            self.last_result = Some(out.result);
                            self.cached_poly_texture = None;
                            self.cached_poly_texture_key = None;
                            self.rect_index =
                                self.last_result.as_ref().map(build_rect_spatial_index);
                            self.fullres_ms = Some(out.elapsed_ms);
                            push_rect_trend(&mut self.rect_count_trend, out.rect_count);
                            self.result_is_preview = false;
                            self.show_live_raster_overlay = false;
                            self.status = "Full-resolution result ready.".to_string();
                            self.status_is_error = false;
                        }
                    }
                }
                WorkerMessage::Failed { epoch, kind, error } => {
                    if epoch != self.latest_epoch {
                        continue;
                    }
                    match kind {
                        JobKind::Preview => {
                            self.pending_preview_epoch = None;
                            self.status = format!("Preview generation failed: {error}");
                        }
                        JobKind::Full => {
                            self.pending_full_epoch = None;
                            self.status = format!("Full-resolution generation failed: {error}");
                            self.show_live_raster_overlay = false;
                        }
                    }
                    self.status_is_error = true;
                }
            }
        }
    }

    fn save_gds(&self) -> Result<()> {
        let result = self
            .last_result
            .as_ref()
            .context("Nothing to save yet; run Generate first")?;
        let gds_layers = vec![LayerRects {
            rects: &result.rects,
            layer: result.layer,
            datatype: result.datatype,
        }];
        write_gds_multi(
            &gds_layers,
            &self.cell_name,
            Path::new(&self.output_path),
            &self.library_name,
            result.pdk.pdk.db_units_per_um,
        )?;
        Ok(())
    }

    fn save_merged_chip_gds(&self) -> Result<()> {
        let result = self
            .last_result
            .as_ref()
            .context("Nothing to merge yet; run Generate first")?;
        anyhow::ensure!(
            !self.chip_gds_path.trim().is_empty(),
            "Chip GDS path is empty; set it in Artwork Placement"
        );

        let (dx, dy) = if self.use_die_bounds {
            let bounds = read_gds_bounds(
                Path::new(&self.chip_gds_path),
                if self.chip_cell_name.trim().is_empty() {
                    None
                } else {
                    Some(self.chip_cell_name.trim())
                },
            )
            .with_context(|| {
                format!(
                    "Failed to read die bounds from chip GDS: {}",
                    self.chip_gds_path
                )
            })?;
            (bounds.x0.0, bounds.y0.0)
        } else {
            (0, 0)
        };
        let shifted_rects: Vec<PolyRect> = if dx == 0 && dy == 0 {
            result.rects.clone()
        } else {
            result
                .rects
                .iter()
                .map(|r| PolyRect::new(r.x0.0 + dx, r.y0.0 + dy, r.x1.0 + dx, r.y1.0 + dy))
                .collect()
        };
        let gds_layers = vec![LayerRects {
            rects: shifted_rects.as_slice(),
            layer: result.layer,
            datatype: result.datatype,
        }];

        merge_into_gds_multi(
            &gds_layers,
            Path::new(&self.chip_gds_path),
            Path::new(&self.output_path),
            if self.chip_cell_name.trim().is_empty() {
                None
            } else {
                Some(self.chip_cell_name.trim())
            },
            0,
            0,
        )?;
        Ok(())
    }

    fn save_svg(&self, path: &Path) -> Result<()> {
        let result = self
            .last_result
            .as_ref()
            .context("Nothing to export yet; run Generate first")?;
        write_svg_multi(
            &[SvgLayer {
                rects: &result.rects,
                color: DEFAULT_LAYER_COLORS[0],
            }],
            path,
            0.01,
            Some("#1a1a2e"),
        )
    }

    fn save_html(&self, path: &Path) -> Result<()> {
        let result = self
            .last_result
            .as_ref()
            .context("Nothing to export yet; run Generate first")?;
        write_html_preview_multi(
            &[HtmlLayer {
                rects: &result.rects,
                name: &result.layer_name,
                color: DEFAULT_LAYER_COLORS[0],
            }],
            path,
            &result.pdk,
        )
    }

    fn save_lef(&self, path: &Path) -> Result<()> {
        let result = self
            .last_result
            .as_ref()
            .context("Nothing to export yet; run Generate first")?;
        write_lef_multi(
            &[LefLayer {
                rects: &result.rects,
                layer_name: &result.layer_name,
            }],
            &result.pdk,
            &self.cell_name,
            path,
        )
    }

    fn builtin_pdk_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.use_custom_pdk, "Use custom PDK TOML");
            if !self.use_custom_pdk {
                egui::ComboBox::from_id_salt("builtin-pdk")
                    .selected_text(self.selected_builtin_pdk.name())
                    .show_ui(ui, |ui| {
                        for pdk in BUILTIN_PDKS {
                            ui.selectable_value(&mut self.selected_builtin_pdk, pdk, pdk.name());
                        }
                    });
            }
        });
    }

    fn custom_pdk_editor_ui(&mut self, ui: &mut egui::Ui) {
        if !self.use_custom_pdk {
            return;
        }
        ui.group(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Template");
                egui::ComboBox::from_id_salt("template-pdk")
                    .selected_text(self.template_builtin_pdk.name())
                    .show_ui(ui, |ui| {
                        for pdk in BUILTIN_PDKS {
                            ui.selectable_value(&mut self.template_builtin_pdk, pdk, pdk.name());
                        }
                    });
                if ui.button("Use Template").clicked() {
                    self.custom_pdk_toml = self.template_builtin_pdk.toml_content().to_string();
                    self.refresh_pdk_validation();
                }
                if ui.button("Upload .toml").clicked()
                    && let Some(path) = FileDialog::new().add_filter("TOML", &["toml"]).pick_file()
                {
                    match std::fs::read_to_string(&path) {
                        Ok(contents) => {
                            self.custom_pdk_toml = contents;
                            self.refresh_pdk_validation();
                            self.status = format!("Loaded custom PDK from {}", path.display());
                            self.status_is_error = false;
                        }
                        Err(e) => {
                            self.status = format!("Failed to read TOML: {e}");
                            self.status_is_error = true;
                        }
                    }
                }
            });
            if ui
                .add(
                    egui::TextEdit::multiline(&mut self.custom_pdk_toml)
                        .font(egui::TextStyle::Monospace)
                        .desired_rows(14),
                )
                .changed()
            {
                self.refresh_pdk_validation();
            }
            if self.pdk_validation.valid {
                ui.colored_label(Color32::from_rgb(20, 130, 20), &self.pdk_validation.message);
            } else {
                ui.colored_label(Color32::from_rgb(180, 30, 30), &self.pdk_validation.message);
            }
        });
    }

    fn controls_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.group(|ui| {
            ui.label("Input");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.input_path).desired_width(280.0));
                if ui.button("Browse...").clicked()
                    && let Some(path) = FileDialog::new()
                        .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "gif", "svg"])
                        .pick_file()
                {
                    self.input_path = path.display().to_string();
                    self.status = format!("Selected image: {}", self.input_path);
                    self.status_is_error = false;
                }
            });
        });

        ui.add_space(6.0);
        self.builtin_pdk_ui(ui);
        self.custom_pdk_editor_ui(ui);

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.label("Pipeline");
            ui.horizontal_wrapped(|ui| {
                ui.label("Threshold");
                ui.add(egui::TextEdit::singleline(&mut self.threshold).desired_width(72.0));
                ui.label("Rotate");
                egui::ComboBox::from_id_salt("rotate")
                    .selected_text(format!("{}", self.rotate))
                    .show_ui(ui, |ui| {
                        for v in [0, 90, 180, 270] {
                            ui.selectable_value(&mut self.rotate, v, v.to_string());
                        }
                    });
                ui.label("Flip");
                egui::ComboBox::from_id_salt("flip")
                    .selected_text(self.flip.as_str())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.flip, Flip::None, Flip::None.as_str());
                        ui.selectable_value(
                            &mut self.flip,
                            Flip::Horizontal,
                            Flip::Horizontal.as_str(),
                        );
                        ui.selectable_value(
                            &mut self.flip,
                            Flip::Vertical,
                            Flip::Vertical.as_str(),
                        );
                    });
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Strategy");
                egui::ComboBox::from_id_salt("strategy")
                    .selected_text(self.strategy.as_str())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.strategy,
                            Strategy::GreedyMerge,
                            Strategy::GreedyMerge.as_str(),
                        );
                        ui.selectable_value(
                            &mut self.strategy,
                            Strategy::HistogramMerge,
                            Strategy::HistogramMerge.as_str(),
                        );
                        ui.selectable_value(
                            &mut self.strategy,
                            Strategy::RowMerge,
                            Strategy::RowMerge.as_str(),
                        );
                        ui.selectable_value(
                            &mut self.strategy,
                            Strategy::PixelRects,
                            Strategy::PixelRects.as_str(),
                        );
                    });
            });
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut self.separated, "Separated");
                ui.checkbox(&mut self.invert, "Invert");
                ui.checkbox(&mut self.dither, "Dither");
                ui.checkbox(&mut self.no_check_drc, "Skip DRC");
                ui.checkbox(&mut self.no_density_enforce, "Skip density enforce");
                ui.checkbox(&mut self.force, "Force despite density failures");
            });
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.label("Artwork Placement");
            ui.horizontal_wrapped(|ui| {
                ui.label("Canvas W");
                ui.add(egui::DragValue::new(&mut self.canvas_width).range(0..=100_000));
                ui.label("Canvas H");
                ui.add(egui::DragValue::new(&mut self.canvas_height).range(0..=100_000));
                ui.label("Image X");
                ui.add(egui::DragValue::new(&mut self.image_x).range(0..=100_000));
                ui.label("Image Y");
                ui.add(egui::DragValue::new(&mut self.image_y).range(0..=100_000));
                ui.label("Scale X %");
                let sx = ui.add(egui::DragValue::new(&mut self.image_scale_x_pct).range(1..=1000));
                ui.label("Scale Y %");
                let sy = ui.add(egui::DragValue::new(&mut self.image_scale_y_pct).range(1..=1000));
                if self.lock_aspect_ratio {
                    if sx.changed() && !sy.changed() {
                        self.image_scale_y_pct = self.image_scale_x_pct;
                    } else if sy.changed() && !sx.changed() {
                        self.image_scale_x_pct = self.image_scale_y_pct;
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut self.lock_aspect_ratio, "Lock aspect ratio");
                if ui.button("Fit Image To Die").clicked() {
                    match self.fit_image_to_die() {
                        Ok(()) => {
                            self.status = "Image fitted to die bounds.".to_string();
                            self.status_is_error = false;
                        }
                        Err(e) => {
                            self.status = format!("Fit-to-die failed: {e:#}");
                            self.status_is_error = true;
                        }
                    }
                }
            });
            ui.small("Canvas 0 means auto-fit to image extents.");
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label("Chip GDS");
                ui.add(egui::TextEdit::singleline(&mut self.chip_gds_path).desired_width(180.0));
                if ui.button("Browse...").clicked()
                    && let Some(path) = FileDialog::new()
                        .add_filter("GDS", &["gds", "gds.gz"])
                        .pick_file()
                {
                    self.chip_gds_path = path.display().to_string();
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Cell (optional)");
                ui.add(egui::TextEdit::singleline(&mut self.chip_cell_name).desired_width(120.0));
                if ui.button("Load Die Bounds").clicked() {
                    if self.chip_gds_path.trim().is_empty() {
                        self.status = "Chip GDS path is empty.".to_string();
                        self.status_is_error = true;
                    } else {
                        match read_gds_bounds(
                            Path::new(&self.chip_gds_path),
                            if self.chip_cell_name.trim().is_empty() {
                                None
                            } else {
                                Some(self.chip_cell_name.trim())
                            },
                        ) {
                            Ok(bb) => {
                                self.die_bounds_dbu = Some(bb);
                                self.use_die_bounds = true;
                                self.status = format!(
                                    "Loaded die bounds: {} x {} dbu",
                                    bb.width().0,
                                    bb.height().0
                                );
                                self.status_is_error = false;
                            }
                            Err(e) => {
                                self.status = format!("Failed to load die bounds: {e:#}");
                                self.status_is_error = true;
                            }
                        }
                    }
                }
                ui.checkbox(&mut self.use_die_bounds, "Use die bounds for canvas");
            });
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.label("Text Overlay");
            ui.add(
                egui::TextEdit::multiline(&mut self.text_overlay)
                    .desired_rows(3)
                    .hint_text("Optional text overlay"),
            );
            ui.horizontal_wrapped(|ui| {
                ui.label("Scale");
                ui.add(egui::DragValue::new(&mut self.text_scale).range(1..=128));
                ui.label("Font");
                egui::ComboBox::from_id_salt("text-font")
                    .selected_text(text_font_str(self.text_font))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.text_font, TextFont::Mono, "mono");
                        ui.selectable_value(
                            &mut self.text_font,
                            TextFont::MonoItalic,
                            "mono-italic",
                        );
                    });
                ui.label("Position");
                overlay_position_combo(ui, "text-position", &mut self.text_position);
                if ui.button("Reset Drag").clicked() {
                    self.text_manual_xy = None;
                }
            });
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.label("QR Overlay");
            ui.add(
                egui::TextEdit::singleline(&mut self.qr_overlay).hint_text("Optional QR payload"),
            );
            ui.horizontal_wrapped(|ui| {
                ui.label("Module size");
                ui.add(egui::DragValue::new(&mut self.qr_module_size).range(1..=128));
                ui.label("EC");
                ec_level_combo(ui, "qr-ec-level", &mut self.qr_ec_level);
                ui.label("Position");
                overlay_position_combo(ui, "qr-position", &mut self.qr_position);
                if ui.button("Reset Drag").clicked() {
                    self.qr_manual_xy = None;
                }
            });
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.label("Overlay Settings");
            ui.horizontal_wrapped(|ui| {
                ui.label("Margin");
                ui.add(egui::DragValue::new(&mut self.overlay_margin).range(0..=1024));
                ui.label("Knockout pad");
                ui.add(egui::DragValue::new(&mut self.overlay_knockout_padding).range(0..=1024));
            });
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.label("Output");
            ui.horizontal(|ui| {
                ui.label("Path");
                ui.add(egui::TextEdit::singleline(&mut self.output_path).desired_width(220.0));
                if ui.button("Browse...").clicked()
                    && let Some(path) = FileDialog::new().add_filter("GDS", &["gds"]).save_file()
                {
                    self.output_path = path.display().to_string();
                }
            });
            ui.horizontal(|ui| {
                ui.label("Cell");
                ui.add(egui::TextEdit::singleline(&mut self.cell_name).desired_width(120.0));
                ui.label("Library");
                ui.add(egui::TextEdit::singleline(&mut self.library_name).desired_width(120.0));
            });
            ui.horizontal_wrapped(|ui| {
                let can_export = self.last_result.is_some() && !self.result_is_preview;
                if ui
                    .add_enabled(can_export, egui::Button::new("Export SVG"))
                    .clicked()
                    && let Some(path) = FileDialog::new().add_filter("SVG", &["svg"]).save_file()
                {
                    match self.save_svg(&path) {
                        Ok(()) => {
                            self.status = format!("Wrote {}", path.display());
                            self.status_is_error = false;
                        }
                        Err(e) => {
                            self.status = format!("SVG export failed: {e:#}");
                            self.status_is_error = true;
                        }
                    }
                }
                if ui
                    .add_enabled(can_export, egui::Button::new("Export HTML"))
                    .clicked()
                    && let Some(path) = FileDialog::new().add_filter("HTML", &["html"]).save_file()
                {
                    match self.save_html(&path) {
                        Ok(()) => {
                            self.status = format!("Wrote {}", path.display());
                            self.status_is_error = false;
                        }
                        Err(e) => {
                            self.status = format!("HTML export failed: {e:#}");
                            self.status_is_error = true;
                        }
                    }
                }
                if ui
                    .add_enabled(can_export, egui::Button::new("Export LEF"))
                    .clicked()
                    && let Some(path) = FileDialog::new().add_filter("LEF", &["lef"]).save_file()
                {
                    match self.save_lef(&path) {
                        Ok(()) => {
                            self.status = format!("Wrote {}", path.display());
                            self.status_is_error = false;
                        }
                        Err(e) => {
                            self.status = format!("LEF export failed: {e:#}");
                            self.status_is_error = true;
                        }
                    }
                }
            });
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.theme_dark, "Dark mode");
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.label("Quality");
            ui.horizontal_wrapped(|ui| {
                ui.label("Preview max dim");
                ui.add(egui::DragValue::new(&mut self.preview_max_dim).range(128..=2048));
                ui.checkbox(&mut self.auto_full_res, "Auto full-res refine");
            });
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.label("Performance");
            ui.label(format!(
                "Preview: {} ms | Full-res: {} ms | Rect trend: {}",
                self.preview_ms
                    .map(|v| format!("{v:.1}"))
                    .unwrap_or_else(|| "n/a".to_string()),
                self.fullres_ms
                    .map(|v| format!("{v:.1}"))
                    .unwrap_or_else(|| "n/a".to_string()),
                if self.rect_count_trend.is_empty() {
                    "n/a".to_string()
                } else {
                    self.rect_count_trend
                        .iter()
                        .map(std::string::ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(" -> ")
                }
            ));
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.label("Selection");
            if let Some(result) = &self.last_result {
                if let Some(idx) = self.selected_rect
                    && let Some(r) = result.rects.get(idx)
                {
                    let dbu = result.pdk.pdk.db_units_per_um as f64;
                    let txt = format!(
                        "Rect #{idx}\n({:.3}, {:.3}) - ({:.3}, {:.3}) um\n{:.3} x {:.3} um",
                        r.x0.0 as f64 / dbu,
                        r.y0.0 as f64 / dbu,
                        r.x1.0 as f64 / dbu,
                        r.y1.0 as f64 / dbu,
                        r.width().0 as f64 / dbu,
                        r.height().0 as f64 / dbu
                    );
                    ui.label(&txt);
                    if ui.button("Copy Coords").clicked() {
                        ui.ctx().copy_text(txt);
                    }
                    if ui.button("Jump To Rect").clicked() {
                        self.focus_rect = Some(*r);
                    }
                } else {
                    ui.label("No polygon selected.");
                }
            } else {
                ui.label("No result.");
            }
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let can_generate =
                self.pending_preview_epoch.is_none() && self.pending_full_epoch.is_none();
            if ui
                .add_enabled(can_generate, egui::Button::new("Generate"))
                .clicked()
            {
                match self.generate(ctx) {
                    Ok(()) => {}
                    Err(e) => {
                        self.status = format!("Generation failed: {e:#}");
                        self.status_is_error = true;
                    }
                }
            }
            if ui
                .add_enabled(
                    self.last_result.is_some() && !self.result_is_preview,
                    egui::Button::new("Save GDS"),
                )
                .clicked()
            {
                match self.save_gds() {
                    Ok(()) => {
                        self.status = format!("Wrote {}", self.output_path);
                        self.status_is_error = false;
                    }
                    Err(e) => {
                        self.status = format!("Save failed: {e:#}");
                        self.status_is_error = true;
                    }
                }
            }
            let can_merge = self.last_result.is_some()
                && !self.result_is_preview
                && !self.chip_gds_path.trim().is_empty();
            if ui
                .add_enabled(can_merge, egui::Button::new("Merge & Save Chip GDS"))
                .clicked()
            {
                match self.save_merged_chip_gds() {
                    Ok(()) => {
                        self.status = format!("Merged artwork into {}", self.output_path);
                        self.status_is_error = false;
                    }
                    Err(e) => {
                        self.status = format!("Merge save failed: {e:#}");
                        self.status_is_error = true;
                    }
                }
            }
        });
    }
}

impl eframe::App for FabbulaGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_worker_results();
        let snap = self.snapshot_settings();
        if self.last_saved_settings.as_ref() != Some(&snap) {
            let _ = save_gui_settings(&snap);
            self.last_saved_settings = Some(snap);
        }
        let current_state = self.autogen_state();
        if !self.interaction_active {
            if self.last_autogen_state.as_ref() != Some(&current_state) {
                self.last_autogen_state = Some(current_state);
                self.auto_regen_deadline =
                    Some(Instant::now() + Duration::from_millis(AUTO_REGEN_DEBOUNCE_MS));
            }
        } else {
            // Freeze auto-regeneration while interacting; regenerate once on release.
            self.auto_regen_deadline = None;
        }
        if let Some(deadline) = self.auto_regen_deadline
            && Instant::now() >= deadline
        {
            self.auto_regen_deadline = None;
            if let Err(e) = self.generate(ctx) {
                self.status = format!("Auto-generate failed: {e:#}");
                self.status_is_error = true;
            }
        }

        if self.theme_dark {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S))
            && self.last_result.is_some()
            && !self.result_is_preview
        {
            match self.save_gds() {
                Ok(()) => {
                    self.status = format!("Wrote {}", self.output_path);
                    self.status_is_error = false;
                }
                Err(e) => {
                    self.status = format!("Save failed: {e:#}");
                    self.status_is_error = true;
                }
            }
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter)) {
            match self.generate(ctx) {
                Ok(()) => {}
                Err(e) => {
                    self.status = format!("Generation failed: {e:#}");
                    self.status_is_error = true;
                }
            }
        }

        egui::SidePanel::left("controls")
            .resizable(true)
            .default_width(440.0)
            .show(ctx, |ui| {
                ui.heading("fabbula gui");
                ui.label("WASM feature parity desktop path");
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| self.controls_ui(ui, ctx));
            });

        egui::TopBottomPanel::bottom("status")
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(&self.stats);
                if self.drc_summary.contains("clean") {
                    ui.colored_label(Color32::from_rgb(20, 130, 20), &self.drc_summary);
                } else if self.drc_summary.contains("violations") {
                    ui.colored_label(Color32::from_rgb(180, 30, 30), &self.drc_summary);
                } else {
                    ui.colored_label(Color32::GRAY, &self.drc_summary);
                }
                if self.status_is_error {
                    ui.colored_label(Color32::from_rgb(180, 30, 30), &self.status);
                } else {
                    ui.colored_label(Color32::GRAY, &self.status);
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            let avail = ui.available_size();
            let (response, painter) = ui.allocate_painter(avail, Sense::drag());
            let viewport = response.rect;
            let bg = if self.theme_dark {
                Color32::from_rgb(13, 17, 23)
            } else {
                Color32::from_rgb(245, 245, 240)
            };
            painter.rect_filled(viewport, 4.0, bg);
            let show_live_overlay = self.interaction_active || self.show_live_raster_overlay;
            let interaction_tex = if show_live_overlay {
                self.interaction_texture(ui.ctx()).ok().flatten()
            } else {
                None
            };
            let interaction_text_tex = if show_live_overlay {
                self.interaction_text_texture(ui.ctx()).ok().flatten()
            } else {
                None
            };
            let interaction_qr_tex = if show_live_overlay {
                self.interaction_qr_texture(ui.ctx()).ok().flatten()
            } else {
                None
            };
            let poly_layer_tex = if !show_live_overlay {
                self.polygon_layer_texture(ui.ctx())
            } else {
                None
            };

            if let Some(result) = &self.last_result {
                if response.hovered() {
                    let scroll_delta = ui.ctx().input(|i| i.raw_scroll_delta.y);
                    if scroll_delta.abs() > f32::EPSILON {
                        let old_zoom = self.preview_zoom;
                        let zoom_factor = (1.0 + scroll_delta * 0.0015).clamp(0.5, 1.5);
                        self.preview_zoom = (self.preview_zoom * zoom_factor).clamp(0.05, 20.0);
                        if let Some(pointer) = response.hover_pos() {
                            let bb = result.full_bb;
                            let old_scale = base_scale(viewport, bb) * old_zoom;
                            let new_scale = base_scale(viewport, bb) * self.preview_zoom;
                            let old_origin = fit_origin(viewport, bb, old_scale) + self.preview_pan;
                            let layout_x = bb.x0.0 as f32 + (pointer.x - old_origin.x) / old_scale;
                            let layout_y = bb.y1.0 as f32 - (pointer.y - old_origin.y) / old_scale;
                            let new_origin = Vec2::new(
                                pointer.x - (layout_x - bb.x0.0 as f32) * new_scale,
                                pointer.y - (bb.y1.0 as f32 - layout_y) * new_scale,
                            );
                            let fit = fit_origin(viewport, bb, new_scale);
                            self.preview_pan = new_origin - fit;
                        }
                    }
                }

                let bb = result.full_bb;
                if let Some(target) = self.focus_rect.take() {
                    let tw = target.width().0.max(1) as f32;
                    let th = target.height().0.max(1) as f32;
                    let fit_scale = (viewport.width() / tw).min(viewport.height() / th) * 0.6;
                    let base = base_scale(viewport, bb).max(1e-6);
                    self.preview_zoom = (fit_scale / base).clamp(0.05, 100.0);
                    let scale_now = base * self.preview_zoom;
                    let fit = fit_origin(viewport, bb, scale_now);
                    let cx = (target.x0.0 + target.x1.0) as f32 * 0.5;
                    let cy = (target.y0.0 + target.y1.0) as f32 * 0.5;
                    let desired = Vec2::new(
                        viewport.center().x - (cx - bb.x0.0 as f32) * scale_now,
                        viewport.center().y - (bb.y1.0 as f32 - cy) * scale_now,
                    );
                    self.preview_pan = desired - fit;
                }
                let scale = base_scale(viewport, bb) * self.preview_zoom;
                let origin = fit_origin(viewport, bb, scale) + self.preview_pan;
                let canvas_bg = if self.theme_dark {
                    Color32::from_rgb(10, 14, 20)
                } else {
                    Color32::from_rgb(238, 238, 232)
                };
                let canvas_rect = to_screen_rect(result.full_bb, bb, origin, scale);
                painter.rect_filled(canvas_rect, 0.0, canvas_bg);
                let selected_stroke = Stroke::new(2.0, Color32::from_rgb(255, 215, 0));
                let hover_stroke = Stroke::new(1.5, Color32::from_rgb(88, 166, 255));

                if !show_live_overlay {
                    let mut hovered = None;
                    if let Some(pointer) = response.hover_pos() {
                        let lx = bb.x0.0 as f32 + (pointer.x - origin.x) / scale;
                        let ly = bb.y1.0 as f32 - (pointer.y - origin.y) / scale;
                        hovered = find_hovered_rect(
                            lx as i32,
                            ly as i32,
                            &result.rects,
                            self.rect_index.as_ref(),
                        );
                    }
                    self.hovered_rect = hovered;
                    if response.clicked() {
                        self.selected_rect = hovered;
                    }

                    if let Some(tex) = &poly_layer_tex {
                        let poly_tint = if self.theme_dark {
                            Color32::WHITE
                        } else {
                            Color32::from_rgb(70, 78, 88)
                        };
                        painter.image(
                            tex.id(),
                            to_screen_rect(result.full_bb, bb, origin, scale),
                            egui::Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                            poly_tint,
                        );
                    }
                    if let Some(idx) = self.selected_rect.or(self.hovered_rect)
                        && let Some(r) = result.rects.get(idx)
                    {
                        let sr = to_screen_rect(*r, bb, origin, scale);
                        let stroke = if Some(idx) == self.selected_rect {
                            selected_stroke
                        } else {
                            hover_stroke
                        };
                        painter.rect_stroke(sr, 0.0, stroke, egui::StrokeKind::Outside);
                    }
                } else {
                    self.hovered_rect = None;
                }

                let handle_half = 4.0;
                let iw = scaled_dim_from_pct(result.source_w_px, self.image_scale_x_pct.max(1));
                let ih = scaled_dim_from_pct(result.source_h_px, self.image_scale_y_pct.max(1));
                let ix = self.image_x;
                let iy = self.image_y;
                let image_layout = bitmap_box_to_layout_rect(
                    ix,
                    iy,
                    iw,
                    ih,
                    result.bitmap_h,
                    result.pitch_dbu,
                    result.pixel_w_dbu,
                );
                let image_screen_rect = to_screen_rect(image_layout, bb, origin, scale);
                if show_live_overlay && let Some(tex) = &interaction_tex {
                    let image_tint = if self.theme_dark {
                        Color32::from_rgba_unmultiplied(255, 255, 255, 200)
                    } else {
                        Color32::from_rgba_unmultiplied(70, 78, 88, 220)
                    };
                    painter.image(
                        tex.id(),
                        image_screen_rect,
                        egui::Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                        image_tint,
                    );
                }

                let mut text_screen_rect: Option<(egui::Rect, u32, u32)> = None;
                let mut text_handle_rects: Option<[(ResizeCorner, egui::Rect); 4]> = None;
                if !self.text_overlay.trim().is_empty() {
                    let tbmp = render_text_with_font(
                        &self.text_overlay,
                        self.text_scale.max(1),
                        0,
                        2,
                        self.text_font,
                    );
                    let (tx, ty) = self.text_manual_xy.unwrap_or_else(|| {
                        self.text_position.place(
                            result.bitmap_w,
                            result.bitmap_h,
                            tbmp.width,
                            tbmp.height,
                            self.overlay_margin,
                        )
                    });
                    let tr = bitmap_box_to_layout_rect(
                        tx,
                        ty,
                        tbmp.width,
                        tbmp.height,
                        result.bitmap_h,
                        result.pitch_dbu,
                        result.pixel_w_dbu,
                    );
                    let ts = to_screen_rect(tr, bb, origin, scale);
                    text_screen_rect = Some((ts, tbmp.width, tbmp.height));
                    if show_live_overlay
                        && let Some(kr) = padded_overlay_screen_rect(
                            tx,
                            ty,
                            tbmp.width,
                            tbmp.height,
                            self.overlay_knockout_padding,
                            result.bitmap_w,
                            result.bitmap_h,
                            result.pitch_dbu,
                            result.pixel_w_dbu,
                            bb,
                            origin,
                            scale,
                        )
                    {
                        painter.rect_filled(kr, 0.0, bg);
                    }
                    if show_live_overlay && let Some(tex) = &interaction_text_tex {
                        painter.image(
                            tex.id(),
                            ts,
                            egui::Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                            Color32::from_rgba_unmultiplied(255, 190, 130, 220),
                        );
                    }
                    painter.rect_stroke(
                        ts,
                        0.0,
                        Stroke::new(1.0, Color32::from_rgb(255, 150, 0)),
                        egui::StrokeKind::Outside,
                    );
                    let handles = image_resize_handle_rects(ts, handle_half);
                    for (_, hrect) in handles {
                        painter.rect_filled(hrect, 1.0, Color32::from_rgb(255, 190, 130));
                        painter.rect_stroke(
                            hrect,
                            1.0,
                            Stroke::new(1.0, Color32::from_rgb(80, 45, 0)),
                            egui::StrokeKind::Outside,
                        );
                    }
                    text_handle_rects = Some(handles);
                }

                let mut qr_screen_rect: Option<(egui::Rect, u32, u32)> = None;
                let mut qr_handle_rects: Option<[(ResizeCorner, egui::Rect); 4]> = None;
                if !self.qr_overlay.trim().is_empty()
                    && let Ok(qbmp) = render_qr(
                        &self.qr_overlay,
                        self.qr_module_size.max(1),
                        self.qr_ec_level,
                        4,
                    )
                {
                    let (qx, qy) = self.qr_manual_xy.unwrap_or_else(|| {
                        self.qr_position.place(
                            result.bitmap_w,
                            result.bitmap_h,
                            qbmp.width,
                            qbmp.height,
                            self.overlay_margin,
                        )
                    });
                    let qr = bitmap_box_to_layout_rect(
                        qx,
                        qy,
                        qbmp.width,
                        qbmp.height,
                        result.bitmap_h,
                        result.pitch_dbu,
                        result.pixel_w_dbu,
                    );
                    let qs = to_screen_rect(qr, bb, origin, scale);
                    qr_screen_rect = Some((qs, qbmp.width, qbmp.height));
                    if show_live_overlay
                        && let Some(kr) = padded_overlay_screen_rect(
                            qx,
                            qy,
                            qbmp.width,
                            qbmp.height,
                            self.overlay_knockout_padding,
                            result.bitmap_w,
                            result.bitmap_h,
                            result.pitch_dbu,
                            result.pixel_w_dbu,
                            bb,
                            origin,
                            scale,
                        )
                    {
                        painter.rect_filled(kr, 0.0, bg);
                    }
                    if show_live_overlay && let Some(tex) = &interaction_qr_tex {
                        painter.image(
                            tex.id(),
                            qs,
                            egui::Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                            Color32::from_rgba_unmultiplied(130, 215, 255, 220),
                        );
                    }
                    painter.rect_stroke(
                        qs,
                        0.0,
                        Stroke::new(1.0, Color32::from_rgb(0, 170, 255)),
                        egui::StrokeKind::Outside,
                    );
                    let handles = image_resize_handle_rects(qs, handle_half);
                    for (_, hrect) in handles {
                        painter.rect_filled(hrect, 1.0, Color32::from_rgb(130, 215, 255));
                        painter.rect_stroke(
                            hrect,
                            1.0,
                            Stroke::new(1.0, Color32::from_rgb(0, 60, 95)),
                            egui::StrokeKind::Outside,
                        );
                    }
                    qr_handle_rects = Some(handles);
                }

                if self.use_die_bounds && self.die_bounds_dbu.is_some() {
                    painter.rect_stroke(
                        to_screen_rect(result.full_bb, bb, origin, scale),
                        0.0,
                        Stroke::new(2.0, Color32::from_rgb(0, 200, 140)),
                        egui::StrokeKind::Outside,
                    );
                } else {
                    let core = default_core_area_rect(result.full_bb);
                    painter.rect_stroke(
                        to_screen_rect(core, bb, origin, scale),
                        0.0,
                        Stroke::new(1.5, Color32::from_rgb(190, 130, 30)),
                        egui::StrokeKind::Outside,
                    );
                }
                painter.rect_stroke(
                    image_screen_rect,
                    0.0,
                    Stroke::new(1.0, Color32::from_rgb(120, 120, 120)),
                    egui::StrokeKind::Outside,
                );
                let handle_rects = image_resize_handle_rects(image_screen_rect, handle_half);
                for (_, hrect) in handle_rects {
                    painter.rect_filled(hrect, 1.0, Color32::from_rgb(230, 230, 230));
                    painter.rect_stroke(
                        hrect,
                        1.0,
                        Stroke::new(1.0, Color32::from_rgb(55, 55, 55)),
                        egui::StrokeKind::Outside,
                    );
                }
                if response.drag_started()
                    && let Some(p) = response.interact_pointer_pos()
                {
                    self.panning_viewport = false;
                    self.interaction_active = false;
                    let lx = bb.x0.0 as f32 + (p.x - origin.x) / scale;
                    let ly = bb.y1.0 as f32 - (p.y - origin.y) / scale;
                    let ptr_px_x = (lx / result.pitch_dbu as f32).round() as i32;
                    let ptr_px_y = (result.bitmap_h as i32 - 1)
                        - (ly / result.pitch_dbu as f32).round() as i32;
                    if let Some(handles) = text_handle_rects
                        && let Some(corner) = hit_test_resize_corner(p, &handles)
                    {
                        self.dragging_overlay = Some(DragOverlay::TextResize(corner));
                        if let Some((_, w, h)) = text_screen_rect {
                            let (tx, ty) = self.text_manual_xy.unwrap_or_else(|| {
                                self.text_position.place(
                                    result.bitmap_w,
                                    result.bitmap_h,
                                    w,
                                    h,
                                    self.overlay_margin,
                                )
                            });
                            let right = tx as i32 + w as i32;
                            let bottom = ty as i32 + h as i32;
                            let anchor = match corner {
                                ResizeCorner::TopLeft => (right, bottom),
                                ResizeCorner::TopRight => (tx as i32, bottom),
                                ResizeCorner::BottomLeft => (right, ty as i32),
                                ResizeCorner::BottomRight => (tx as i32, ty as i32),
                            };
                            self.text_resize_drag = Some(UniformResizeDrag {
                                corner,
                                anchor_px: anchor,
                                start_w: w,
                                start_h: h,
                                start_value: self.text_scale.max(1),
                            });
                        }
                    } else if let Some((rect, _, _)) = text_screen_rect
                        && rect.contains(p)
                    {
                        self.dragging_overlay = Some(DragOverlay::Text);
                        if let Some((_, w, h)) = text_screen_rect {
                            let (tx, ty) = self.text_manual_xy.unwrap_or_else(|| {
                                self.text_position.place(
                                    result.bitmap_w,
                                    result.bitmap_h,
                                    w,
                                    h,
                                    self.overlay_margin,
                                )
                            });
                            self.drag_offset_px = (ptr_px_x - tx as i32, ptr_px_y - ty as i32);
                        }
                    } else if let Some(handles) = qr_handle_rects
                        && let Some(corner) = hit_test_resize_corner(p, &handles)
                    {
                        self.dragging_overlay = Some(DragOverlay::QrResize(corner));
                        if let Some((_, w, h)) = qr_screen_rect {
                            let (qx, qy) = self.qr_manual_xy.unwrap_or_else(|| {
                                self.qr_position.place(
                                    result.bitmap_w,
                                    result.bitmap_h,
                                    w,
                                    h,
                                    self.overlay_margin,
                                )
                            });
                            let right = qx as i32 + w as i32;
                            let bottom = qy as i32 + h as i32;
                            let anchor = match corner {
                                ResizeCorner::TopLeft => (right, bottom),
                                ResizeCorner::TopRight => (qx as i32, bottom),
                                ResizeCorner::BottomLeft => (right, qy as i32),
                                ResizeCorner::BottomRight => (qx as i32, qy as i32),
                            };
                            self.qr_resize_drag = Some(UniformResizeDrag {
                                corner,
                                anchor_px: anchor,
                                start_w: w,
                                start_h: h,
                                start_value: self.qr_module_size.max(1),
                            });
                        }
                    } else if let Some((rect, _, _)) = qr_screen_rect
                        && rect.contains(p)
                    {
                        self.dragging_overlay = Some(DragOverlay::Qr);
                        if let Some((_, w, h)) = qr_screen_rect {
                            let (qx, qy) = self.qr_manual_xy.unwrap_or_else(|| {
                                self.qr_position.place(
                                    result.bitmap_w,
                                    result.bitmap_h,
                                    w,
                                    h,
                                    self.overlay_margin,
                                )
                            });
                            self.drag_offset_px = (ptr_px_x - qx as i32, ptr_px_y - qy as i32);
                        }
                    } else if let Some(corner) = hit_test_resize_corner(p, &handle_rects) {
                        self.dragging_overlay = Some(DragOverlay::ImageResize(corner));
                        let right = self.image_x as i32 + iw as i32;
                        let bottom = self.image_y as i32 + ih as i32;
                        let anchor = match corner {
                            ResizeCorner::TopLeft => (right, bottom),
                            ResizeCorner::TopRight => (self.image_x as i32, bottom),
                            ResizeCorner::BottomLeft => (right, self.image_y as i32),
                            ResizeCorner::BottomRight => (self.image_x as i32, self.image_y as i32),
                        };
                        self.image_resize_drag = Some(ImageResizeDrag {
                            corner,
                            anchor_px: anchor,
                            start_image_w: iw,
                            start_image_h: ih,
                            start_scale_x_pct: self.image_scale_x_pct.max(1),
                            start_scale_y_pct: self.image_scale_y_pct.max(1),
                        });
                    } else if image_screen_rect.contains(p) {
                        self.dragging_overlay = Some(DragOverlay::ImageMove);
                        self.drag_offset_px = (
                            ptr_px_x - self.image_x as i32,
                            ptr_px_y - self.image_y as i32,
                        );
                    } else {
                        self.panning_viewport = true;
                    }
                    if self.dragging_overlay.is_some() {
                        if !self.use_die_bounds && self.canvas_width == 0 && self.canvas_height == 0
                        {
                            self.canvas_width = result.bitmap_w.max(1);
                            self.canvas_height = result.bitmap_h.max(1);
                        }
                        self.interaction_active = true;
                        self.show_live_raster_overlay = true;
                    }
                }
                if response.drag_stopped() {
                    self.dragging_overlay = None;
                    self.image_resize_drag = None;
                    self.text_resize_drag = None;
                    self.qr_resize_drag = None;
                    self.panning_viewport = false;
                    if self.interaction_active {
                        self.interaction_active = false;
                        self.auto_regen_deadline = Some(
                            Instant::now() + Duration::from_millis(INTERACTION_REGEN_DEBOUNCE_MS),
                        );
                    }
                }
                if let Some(kind) = self.dragging_overlay
                    && let Some(p) = response.interact_pointer_pos()
                {
                    let lx = bb.x0.0 as f32 + (p.x - origin.x) / scale;
                    let ly = bb.y1.0 as f32 - (p.y - origin.y) / scale;
                    let px = (lx / result.pitch_dbu as f32).round() as i32;
                    let py = (result.bitmap_h as i32 - 1)
                        - (ly / result.pitch_dbu as f32).round() as i32;
                    let anchor_x = px - self.drag_offset_px.0;
                    let anchor_y = py - self.drag_offset_px.1;
                    match kind {
                        DragOverlay::ImageMove => {
                            let (w, h) = (iw, ih);
                            self.image_x =
                                anchor_x.clamp(0, result.bitmap_w.saturating_sub(w) as i32) as u32;
                            self.image_y =
                                anchor_y.clamp(0, result.bitmap_h.saturating_sub(h) as i32) as u32;
                        }
                        DragOverlay::Text => {
                            if let Some((_, w, h)) = text_screen_rect {
                                self.text_manual_xy = Some((
                                    anchor_x.clamp(0, result.bitmap_w.saturating_sub(w) as i32)
                                        as u32,
                                    anchor_y.clamp(0, result.bitmap_h.saturating_sub(h) as i32)
                                        as u32,
                                ));
                            }
                        }
                        DragOverlay::TextResize(corner) => {
                            if let Some(drag) = self.text_resize_drag
                                && drag.corner == corner
                            {
                                let mut new_w = (drag.anchor_px.0 - px).unsigned_abs().max(1);
                                let mut new_h = (drag.anchor_px.1 - py).unsigned_abs().max(1);
                                let fx = new_w as f64 / drag.start_w.max(1) as f64;
                                let fy = new_h as f64 / drag.start_h.max(1) as f64;
                                let f = fx.max(fy);
                                let new_scale =
                                    ((drag.start_value as f64 * f).round() as u32).max(1);
                                self.text_scale = new_scale;
                                let tbmp = render_text_with_font(
                                    &self.text_overlay,
                                    self.text_scale.max(1),
                                    0,
                                    2,
                                    self.text_font,
                                );
                                new_w = tbmp.width.max(1);
                                new_h = tbmp.height.max(1);
                                let (new_x, new_y) = match corner {
                                    ResizeCorner::TopLeft => (
                                        drag.anchor_px.0 - new_w as i32,
                                        drag.anchor_px.1 - new_h as i32,
                                    ),
                                    ResizeCorner::TopRight => {
                                        (drag.anchor_px.0, drag.anchor_px.1 - new_h as i32)
                                    }
                                    ResizeCorner::BottomLeft => {
                                        (drag.anchor_px.0 - new_w as i32, drag.anchor_px.1)
                                    }
                                    ResizeCorner::BottomRight => {
                                        (drag.anchor_px.0, drag.anchor_px.1)
                                    }
                                };
                                let max_x = result.bitmap_w.saturating_sub(new_w) as i32;
                                let max_y = result.bitmap_h.saturating_sub(new_h) as i32;
                                self.text_manual_xy = Some((
                                    new_x.clamp(0, max_x) as u32,
                                    new_y.clamp(0, max_y) as u32,
                                ));
                            }
                        }
                        DragOverlay::Qr => {
                            if let Some((_, w, h)) = qr_screen_rect {
                                self.qr_manual_xy = Some((
                                    anchor_x.clamp(0, result.bitmap_w.saturating_sub(w) as i32)
                                        as u32,
                                    anchor_y.clamp(0, result.bitmap_h.saturating_sub(h) as i32)
                                        as u32,
                                ));
                            }
                        }
                        DragOverlay::QrResize(corner) => {
                            if let Some(drag) = self.qr_resize_drag
                                && drag.corner == corner
                            {
                                let new_w = (drag.anchor_px.0 - px).unsigned_abs().max(1);
                                let new_h = (drag.anchor_px.1 - py).unsigned_abs().max(1);
                                let fx = new_w as f64 / drag.start_w.max(1) as f64;
                                let fy = new_h as f64 / drag.start_h.max(1) as f64;
                                let f = fx.max(fy);
                                self.qr_module_size =
                                    ((drag.start_value as f64 * f).round() as u32).max(1);
                                if let Ok(qbmp) = render_qr(
                                    &self.qr_overlay,
                                    self.qr_module_size.max(1),
                                    self.qr_ec_level,
                                    4,
                                ) {
                                    let (new_w, new_h) = (qbmp.width.max(1), qbmp.height.max(1));
                                    let (new_x, new_y) = match corner {
                                        ResizeCorner::TopLeft => (
                                            drag.anchor_px.0 - new_w as i32,
                                            drag.anchor_px.1 - new_h as i32,
                                        ),
                                        ResizeCorner::TopRight => {
                                            (drag.anchor_px.0, drag.anchor_px.1 - new_h as i32)
                                        }
                                        ResizeCorner::BottomLeft => {
                                            (drag.anchor_px.0 - new_w as i32, drag.anchor_px.1)
                                        }
                                        ResizeCorner::BottomRight => {
                                            (drag.anchor_px.0, drag.anchor_px.1)
                                        }
                                    };
                                    let max_x = result.bitmap_w.saturating_sub(new_w) as i32;
                                    let max_y = result.bitmap_h.saturating_sub(new_h) as i32;
                                    self.qr_manual_xy = Some((
                                        new_x.clamp(0, max_x) as u32,
                                        new_y.clamp(0, max_y) as u32,
                                    ));
                                }
                            }
                        }
                        DragOverlay::ImageResize(corner) => {
                            if let Some(drag) = self.image_resize_drag
                                && drag.corner == corner
                            {
                                let (mut new_x, mut new_y, mut new_w, mut new_h) = match corner {
                                    ResizeCorner::TopLeft => {
                                        let nx = px.min(drag.anchor_px.0 - 1);
                                        let ny = py.min(drag.anchor_px.1 - 1);
                                        (
                                            nx,
                                            ny,
                                            (drag.anchor_px.0 - nx).max(1) as u32,
                                            (drag.anchor_px.1 - ny).max(1) as u32,
                                        )
                                    }
                                    ResizeCorner::TopRight => {
                                        let nx = drag.anchor_px.0;
                                        let ny = py.min(drag.anchor_px.1 - 1);
                                        let right = px.max(nx + 1);
                                        (
                                            nx,
                                            ny,
                                            (right - nx).max(1) as u32,
                                            (drag.anchor_px.1 - ny).max(1) as u32,
                                        )
                                    }
                                    ResizeCorner::BottomLeft => {
                                        let nx = px.min(drag.anchor_px.0 - 1);
                                        let ny = drag.anchor_px.1;
                                        let bottom = py.max(ny + 1);
                                        (
                                            nx,
                                            ny,
                                            (drag.anchor_px.0 - nx).max(1) as u32,
                                            (bottom - ny).max(1) as u32,
                                        )
                                    }
                                    ResizeCorner::BottomRight => {
                                        let nx = drag.anchor_px.0;
                                        let ny = drag.anchor_px.1;
                                        let right = px.max(nx + 1);
                                        let bottom = py.max(ny + 1);
                                        (
                                            nx,
                                            ny,
                                            (right - nx).max(1) as u32,
                                            (bottom - ny).max(1) as u32,
                                        )
                                    }
                                };
                                if self.lock_aspect_ratio {
                                    let fx = new_w as f64 / drag.start_image_w.max(1) as f64;
                                    let fy = new_h as f64 / drag.start_image_h.max(1) as f64;
                                    let f = fx.max(fy).max(0.01);
                                    new_w = (drag.start_image_w as f64 * f).round().max(1.0) as u32;
                                    new_h = (drag.start_image_h as f64 * f).round().max(1.0) as u32;
                                    match corner {
                                        ResizeCorner::TopLeft => {
                                            new_x = drag.anchor_px.0 - new_w as i32;
                                            new_y = drag.anchor_px.1 - new_h as i32;
                                        }
                                        ResizeCorner::TopRight => {
                                            new_x = drag.anchor_px.0;
                                            new_y = drag.anchor_px.1 - new_h as i32;
                                        }
                                        ResizeCorner::BottomLeft => {
                                            new_x = drag.anchor_px.0 - new_w as i32;
                                            new_y = drag.anchor_px.1;
                                        }
                                        ResizeCorner::BottomRight => {
                                            new_x = drag.anchor_px.0;
                                            new_y = drag.anchor_px.1;
                                        }
                                    }
                                }
                                let max_x = result.bitmap_w.saturating_sub(new_w) as i32;
                                let max_y = result.bitmap_h.saturating_sub(new_h) as i32;
                                self.image_x = new_x.clamp(0, max_x) as u32;
                                self.image_y = new_y.clamp(0, max_y) as u32;
                                let fx = new_w as f64 / drag.start_image_w.max(1) as f64;
                                let fy = new_h as f64 / drag.start_image_h.max(1) as f64;
                                if self.lock_aspect_ratio {
                                    let f = fx.max(fy);
                                    let new_scale =
                                        ((drag.start_scale_x_pct as f64 * f).round() as u32).max(1);
                                    self.image_scale_x_pct = new_scale;
                                    self.image_scale_y_pct = new_scale;
                                } else {
                                    self.image_scale_x_pct =
                                        ((drag.start_scale_x_pct as f64 * fx).round() as u32)
                                            .max(1);
                                    self.image_scale_y_pct =
                                        ((drag.start_scale_y_pct as f64 * fy).round() as u32)
                                            .max(1);
                                }
                            }
                        }
                    }
                }
                if self.panning_viewport && response.dragged() {
                    self.preview_pan += ui.ctx().input(|i| i.pointer.delta());
                    ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                } else if let Some(kind) = self.dragging_overlay {
                    let icon = match kind {
                        DragOverlay::ImageMove => CursorIcon::Move,
                        DragOverlay::Text | DragOverlay::Qr => CursorIcon::Move,
                        DragOverlay::TextResize(ResizeCorner::TopLeft)
                        | DragOverlay::TextResize(ResizeCorner::BottomRight)
                        | DragOverlay::QrResize(ResizeCorner::TopLeft)
                        | DragOverlay::QrResize(ResizeCorner::BottomRight)
                        | DragOverlay::ImageResize(ResizeCorner::TopLeft)
                        | DragOverlay::ImageResize(ResizeCorner::BottomRight) => {
                            CursorIcon::ResizeNwSe
                        }
                        DragOverlay::TextResize(ResizeCorner::TopRight)
                        | DragOverlay::TextResize(ResizeCorner::BottomLeft)
                        | DragOverlay::QrResize(ResizeCorner::TopRight)
                        | DragOverlay::QrResize(ResizeCorner::BottomLeft)
                        | DragOverlay::ImageResize(ResizeCorner::TopRight)
                        | DragOverlay::ImageResize(ResizeCorner::BottomLeft) => {
                            CursorIcon::ResizeNeSw
                        }
                    };
                    ui.ctx().set_cursor_icon(icon);
                } else if let Some(p) = response.hover_pos() {
                    if let Some(handles) = text_handle_rects
                        && let Some(corner) = hit_test_resize_corner(p, &handles)
                    {
                        let icon = match corner {
                            ResizeCorner::TopLeft | ResizeCorner::BottomRight => {
                                CursorIcon::ResizeNwSe
                            }
                            ResizeCorner::TopRight | ResizeCorner::BottomLeft => {
                                CursorIcon::ResizeNeSw
                            }
                        };
                        ui.ctx().set_cursor_icon(icon);
                    } else if let Some(handles) = qr_handle_rects
                        && let Some(corner) = hit_test_resize_corner(p, &handles)
                    {
                        let icon = match corner {
                            ResizeCorner::TopLeft | ResizeCorner::BottomRight => {
                                CursorIcon::ResizeNwSe
                            }
                            ResizeCorner::TopRight | ResizeCorner::BottomLeft => {
                                CursorIcon::ResizeNeSw
                            }
                        };
                        ui.ctx().set_cursor_icon(icon);
                    } else if let Some(corner) = hit_test_resize_corner(p, &handle_rects) {
                        let icon = match corner {
                            ResizeCorner::TopLeft | ResizeCorner::BottomRight => {
                                CursorIcon::ResizeNwSe
                            }
                            ResizeCorner::TopRight | ResizeCorner::BottomLeft => {
                                CursorIcon::ResizeNeSw
                            }
                        };
                        ui.ctx().set_cursor_icon(icon);
                    } else if text_screen_rect.is_some_and(|(r, _, _)| r.contains(p))
                        || qr_screen_rect.is_some_and(|(r, _, _)| r.contains(p))
                        || image_screen_rect.contains(p)
                    {
                        ui.ctx().set_cursor_icon(CursorIcon::Move);
                    } else if response.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::Grab);
                    }
                }

                if let (Some(idx), Some(pointer)) = (self.hovered_rect, response.hover_pos())
                    && let Some(r) = result.rects.get(idx)
                {
                    let dbu = result.pdk.pdk.db_units_per_um as f64;
                    let tip = format!(
                        "Rect #{idx}\n({:.3}, {:.3}) - ({:.3}, {:.3}) um\n{:.3} x {:.3} um",
                        r.x0.0 as f64 / dbu,
                        r.y0.0 as f64 / dbu,
                        r.x1.0 as f64 / dbu,
                        r.y1.0 as f64 / dbu,
                        r.width().0 as f64 / dbu,
                        r.height().0 as f64 / dbu
                    );
                    let pad = Vec2::new(8.0, 6.0);
                    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
                    let galley =
                        painter.layout_no_wrap(tip.clone(), font_id, ui.visuals().text_color());
                    let pos = pointer + Vec2::new(12.0, 12.0);
                    let tip_rect = egui::Rect::from_min_size(pos, galley.size() + pad * 2.0);
                    let panel_bg = if self.theme_dark {
                        Color32::from_rgb(22, 27, 34)
                    } else {
                        Color32::from_rgb(255, 255, 255)
                    };
                    let panel_border = if self.theme_dark {
                        Color32::from_rgb(48, 54, 61)
                    } else {
                        Color32::from_rgb(208, 215, 222)
                    };
                    painter.rect_filled(tip_rect, 6.0, panel_bg);
                    painter.rect_stroke(
                        tip_rect,
                        6.0,
                        Stroke::new(1.0, panel_border),
                        egui::StrokeKind::Outside,
                    );
                    painter.galley(pos + pad, galley, ui.visuals().text_color());
                }
            } else {
                painter.text(
                    viewport.center(),
                    egui::Align2::CENTER_CENTER,
                    "No preview yet.",
                    egui::TextStyle::Body.resolve(ui.style()),
                    Color32::from_gray(90),
                );
            }
        });

        if let Some(result) = &self.last_result
            && let Some(idx) = self.selected_rect.or(self.hovered_rect)
            && let Some(r) = result.rects.get(idx)
        {
            let dbu = result.pdk.pdk.db_units_per_um as f64;
            self.status = format!(
                "Rect #{idx}: ({:.3}, {:.3})-({:.3}, {:.3}) um | {:.3} x {:.3} um",
                r.x0.0 as f64 / dbu,
                r.y0.0 as f64 / dbu,
                r.x1.0 as f64 / dbu,
                r.y1.0 as f64 / dbu,
                r.width().0 as f64 / dbu,
                r.height().0 as f64 / dbu
            );
            self.status_is_error = false;
        }
    }
}

fn downsample_bitmap_nearest(src: &ArtworkBitmap, max_dim: u32) -> ArtworkBitmap {
    let max_wh = src.width.max(src.height);
    if max_wh <= max_dim || src.width == 0 || src.height == 0 {
        return src.clone();
    }
    let scale = max_dim as f64 / max_wh as f64;
    let out_w = ((src.width as f64 * scale).round() as u32).max(1);
    let out_h = ((src.height as f64 * scale).round() as u32).max(1);
    #[cfg(feature = "rayon")]
    {
        let rows: Vec<Vec<bool>> = (0..out_h)
            .into_par_iter()
            .map(|y| {
                let sy = ((y as f64 / out_h as f64) * src.height as f64)
                    .floor()
                    .min((src.height - 1) as f64) as u32;
                let mut row = vec![false; out_w as usize];
                for x in 0..out_w {
                    let sx = ((x as f64 / out_w as f64) * src.width as f64)
                        .floor()
                        .min((src.width - 1) as f64) as u32;
                    row[x as usize] = src.get(sx, sy);
                }
                row
            })
            .collect();
        let mut out = ArtworkBitmap::new_zeroed(out_w, out_h);
        for y in 0..out_h {
            let row = &rows[y as usize];
            for x in 0..out_w {
                out.set(x, y, row[x as usize]);
            }
        }
        out
    }
    #[cfg(not(feature = "rayon"))]
    {
        let mut out = ArtworkBitmap::new_zeroed(out_w, out_h);
        for y in 0..out_h {
            let sy = ((y as f64 / out_h as f64) * src.height as f64)
                .floor()
                .min((src.height - 1) as f64) as u32;
            for x in 0..out_w {
                let sx = ((x as f64 / out_w as f64) * src.width as f64)
                    .floor()
                    .min((src.width - 1) as f64) as u32;
                out.set(x, y, src.get(sx, sy));
            }
        }
        out
    }
}

fn scale_bitmap_nearest_xy(src: &ArtworkBitmap, scale_x: f64, scale_y: f64) -> ArtworkBitmap {
    let out_w = ((src.width as f64 * scale_x).round() as u32).max(1);
    let out_h = ((src.height as f64 * scale_y).round() as u32).max(1);
    #[cfg(feature = "rayon")]
    {
        let rows: Vec<Vec<bool>> = (0..out_h)
            .into_par_iter()
            .map(|y| {
                let sy = ((y as f64 / out_h as f64) * src.height as f64)
                    .floor()
                    .min((src.height - 1) as f64) as u32;
                let mut row = vec![false; out_w as usize];
                for x in 0..out_w {
                    let sx = ((x as f64 / out_w as f64) * src.width as f64)
                        .floor()
                        .min((src.width - 1) as f64) as u32;
                    row[x as usize] = src.get(sx, sy);
                }
                row
            })
            .collect();
        let mut out = ArtworkBitmap::new_zeroed(out_w, out_h);
        for y in 0..out_h {
            let row = &rows[y as usize];
            for x in 0..out_w {
                out.set(x, y, row[x as usize]);
            }
        }
        out
    }
    #[cfg(not(feature = "rayon"))]
    {
        let mut out = ArtworkBitmap::new_zeroed(out_w, out_h);
        for y in 0..out_h {
            let sy = ((y as f64 / out_h as f64) * src.height as f64)
                .floor()
                .min((src.height - 1) as f64) as u32;
            for x in 0..out_w {
                let sx = ((x as f64 / out_w as f64) * src.width as f64)
                    .floor()
                    .min((src.width - 1) as f64) as u32;
                out.set(x, y, src.get(sx, sy));
            }
        }
        out
    }
}

fn apply_bitmap_transforms(bitmap: &mut ArtworkBitmap, cfg: &GenerationConfig) -> Result<()> {
    if cfg.invert {
        bitmap.invert();
    }
    anyhow::ensure!(
        matches!(cfg.rotate, 0 | 90 | 180 | 270),
        "Rotate must be 0/90/180/270"
    );
    if cfg.rotate != 0 {
        bitmap.rotate(cfg.rotate);
    }
    match cfg.flip {
        Flip::Horizontal => bitmap.flip_horizontal(),
        Flip::Vertical => bitmap.flip_vertical(),
        Flip::None => {}
    }
    Ok(())
}

fn apply_overlays(bitmap: &mut ArtworkBitmap, cfg: &GenerationConfig) -> Result<()> {
    if !cfg.text_overlay.trim().is_empty() {
        let text_bitmap = render_text_with_font(
            &cfg.text_overlay,
            cfg.text_scale.max(1),
            0,
            2,
            cfg.text_font,
        );
        let (x, y) = cfg.text_manual_xy.unwrap_or_else(|| {
            cfg.text_position.place(
                bitmap.width,
                bitmap.height,
                text_bitmap.width,
                text_bitmap.height,
                cfg.overlay_margin,
            )
        });
        clear_knockout_rect(
            bitmap,
            x,
            y,
            text_bitmap.width,
            text_bitmap.height,
            cfg.overlay_knockout_padding,
        );
        bitmap.composite(&text_bitmap, x, y);
    }
    if !cfg.qr_overlay.trim().is_empty() {
        let qr_bitmap = render_qr(
            &cfg.qr_overlay,
            cfg.qr_module_size.max(1),
            cfg.qr_ec_level,
            4,
        )?;
        let (x, y) = cfg.qr_manual_xy.unwrap_or_else(|| {
            cfg.qr_position.place(
                bitmap.width,
                bitmap.height,
                qr_bitmap.width,
                qr_bitmap.height,
                cfg.overlay_margin,
            )
        });
        clear_knockout_rect(
            bitmap,
            x,
            y,
            qr_bitmap.width,
            qr_bitmap.height,
            cfg.overlay_knockout_padding,
        );
        bitmap.composite(&qr_bitmap, x, y);
    }
    Ok(())
}

fn clear_knockout_rect(bitmap: &mut ArtworkBitmap, x: u32, y: u32, w: u32, h: u32, padding: u32) {
    let x0 = x.saturating_sub(padding);
    let y0 = y.saturating_sub(padding);
    let x1 = x
        .saturating_add(w)
        .saturating_add(padding)
        .min(bitmap.width);
    let y1 = y
        .saturating_add(h)
        .saturating_add(padding)
        .min(bitmap.height);
    for yy in y0..y1 {
        for xx in x0..x1 {
            bitmap.set(xx, yy, false);
        }
    }
}

fn run_generation_job(
    mut bitmap: ArtworkBitmap,
    cfg: &GenerationConfig,
    fast_preview: bool,
) -> Result<JobUiOutput> {
    let t0 = Instant::now();
    let pdk = if cfg.use_custom_pdk {
        PdkConfig::from_toml_str(&cfg.custom_pdk_toml)?
    } else {
        PdkConfig::builtin(cfg.selected_builtin_pdk.name())?
    };
    let profile = &pdk.layer_profiles()[0];
    let placement = if cfg.separated {
        PixelPlacement::Separated
    } else {
        PixelPlacement::Touching
    };
    let min_w_um = pdk.snap_to_grid(profile.drc.min_width);
    let eff_s_um = pdk.snap_to_grid(profile.drc.effective_spacing());
    let touching = placement == PixelPlacement::Touching;
    let pitch_um = if touching {
        min_w_um.max(eff_s_um)
    } else {
        min_w_um + eff_s_um
    };
    let pixel_w_um = if touching { pitch_um } else { min_w_um };
    let pitch_dbu = pdk.um_to_dbu(pitch_um).0;
    let pixel_w_dbu = pdk.um_to_dbu(pixel_w_um).0;

    apply_bitmap_transforms(&mut bitmap, cfg)?;
    let source_w_px = bitmap.width.max(1);
    let source_h_px = bitmap.height.max(1);
    let scale_x = cfg.image_scale_x_pct.max(1) as f64 / 100.0;
    let scale_y = cfg.image_scale_y_pct.max(1) as f64 / 100.0;
    let scaled = if (scale_x - 1.0).abs() > f64::EPSILON || (scale_y - 1.0).abs() > f64::EPSILON {
        scale_bitmap_nearest_xy(&bitmap, scale_x, scale_y)
    } else {
        bitmap
    };
    let image_x = cfg.image_x;
    let image_y = cfg.image_y;
    let auto_w = image_x.saturating_add(scaled.width);
    let auto_h = image_y.saturating_add(scaled.height);
    let (canvas_w, canvas_h) = if cfg.use_die_bounds {
        if let Some(bb) = cfg.die_bounds_dbu {
            let w = bb.width().0.max(1);
            let h = bb.height().0.max(1);
            let px_w = ((w + pitch_dbu - 1) / pitch_dbu).max(1) as u32;
            let px_h = ((h + pitch_dbu - 1) / pitch_dbu).max(1) as u32;
            (px_w, px_h)
        } else {
            (
                cfg.canvas_width.max(auto_w).max(1),
                cfg.canvas_height.max(auto_h).max(1),
            )
        }
    } else {
        // Explicit non-zero canvas dimensions are treated as fixed.
        let cw = if cfg.canvas_width == 0 {
            auto_w.max(1)
        } else {
            cfg.canvas_width.max(1)
        };
        let ch = if cfg.canvas_height == 0 {
            auto_h.max(1)
        } else {
            cfg.canvas_height.max(1)
        };
        (cw, ch)
    };
    let mut bitmap = ArtworkBitmap::new_zeroed(canvas_w, canvas_h);
    bitmap.composite(&scaled, image_x, image_y);
    apply_overlays(&mut bitmap, cfg)?;

    let rects = generate_layer_polygons(
        &mut bitmap,
        &pdk,
        &profile.drc,
        cfg.strategy.to_polygon(),
        placement,
        !cfg.no_density_enforce && !fast_preview,
        cfg.force,
    )?;
    let violations = if cfg.no_check_drc || fast_preview {
        Vec::new()
    } else {
        check_drc(&rects, pdk.pdk.db_units_per_um, &profile.drc)
    };

    let bb = bounding_box(&rects).unwrap_or(PolyRect::new(0, 0, 0, 0));
    let dbu_per_um = pdk.pdk.db_units_per_um as f64;
    let stats = format!(
        "PDK: {} | Bitmap: {}x{} px{} | Density: {:.1}% | Polygons: {} | Size: {:.2}um x {:.2}um",
        if cfg.use_custom_pdk {
            "custom"
        } else {
            cfg.selected_builtin_pdk.name()
        },
        bitmap.width,
        bitmap.height,
        if fast_preview { " (preview)" } else { "" },
        bitmap.density() * 100.0,
        rects.len(),
        bb.width().0 as f64 / dbu_per_um,
        bb.height().0 as f64 / dbu_per_um,
    );
    let drc_summary = if cfg.no_check_drc || fast_preview {
        if fast_preview {
            "DRC skipped in preview mode".to_string()
        } else {
            "DRC skipped (--no-check-drc)".to_string()
        }
    } else if violations.is_empty() {
        "DRC clean".to_string()
    } else {
        format!("DRC violations: {}", violations.len())
    };
    let full_w = ((bitmap.width.saturating_sub(1)) as i64 * pitch_dbu as i64 + pixel_w_dbu as i64)
        .max(1)
        .min(i32::MAX as i64) as i32;
    let full_h = ((bitmap.height.saturating_sub(1)) as i64 * pitch_dbu as i64 + pixel_w_dbu as i64)
        .max(1)
        .min(i32::MAX as i64) as i32;
    let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let rect_count = rects.len();
    Ok(JobUiOutput {
        result: GuiResult {
            rects,
            pdk: pdk.clone(),
            layer_name: profile.name.clone(),
            layer: profile.gds_layer,
            datatype: profile.gds_datatype,
            full_bb: PolyRect::new(0, 0, full_w, full_h),
            bitmap_w: bitmap.width,
            bitmap_h: bitmap.height,
            pitch_dbu,
            pixel_w_dbu,
            source_w_px,
            source_h_px,
        },
        stats,
        drc_summary,
        elapsed_ms,
        rect_count,
    })
}

fn spawn_generation_worker(
    rx: Receiver<GenerationJob>,
    tx: Sender<WorkerMessage>,
    preview_worker: bool,
    latest_epoch: Arc<AtomicU64>,
) {
    thread::spawn(move || {
        while let Ok(job) = rx.recv() {
            let mut latest_job = job;
            while let Ok(next_job) = rx.try_recv() {
                latest_job = next_job;
            }
            if latest_job.epoch < latest_epoch.load(Ordering::Relaxed) {
                continue;
            }
            let fast_preview = preview_worker;
            let msg = match run_generation_job(latest_job.bitmap, &latest_job.cfg, fast_preview) {
                Ok(out) => WorkerMessage::Done {
                    epoch: latest_job.epoch,
                    kind: latest_job.kind,
                    out: Box::new(out),
                },
                Err(e) => WorkerMessage::Failed {
                    epoch: latest_job.epoch,
                    kind: latest_job.kind,
                    error: e.to_string(),
                },
            };
            let _ = tx.send(msg);
        }
    });
}

fn push_rect_trend(trend: &mut VecDeque<usize>, count: usize) {
    if trend.len() >= 32 {
        trend.pop_front();
    }
    trend.push_back(count);
}

fn settings_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".fabbula_gui.toml"))
}

fn default_lock_aspect_ratio() -> bool {
    true
}

fn default_image_scale_pct() -> u32 {
    100
}

fn load_gui_settings() -> Option<GuiSettings> {
    let path = settings_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    toml::from_str::<GuiSettings>(&text).ok()
}

fn save_gui_settings(settings: &GuiSettings) -> Result<()> {
    let path = settings_path().context("No HOME for settings path")?;
    let text = toml::to_string(settings)?;
    std::fs::write(path, text)?;
    Ok(())
}

fn parse_overlay_position(s: &str) -> Option<OverlayPosition> {
    match s {
        "top" => Some(OverlayPosition::Top),
        "bottom" => Some(OverlayPosition::Bottom),
        "top-left" => Some(OverlayPosition::TopLeft),
        "top-right" => Some(OverlayPosition::TopRight),
        "bottom-left" => Some(OverlayPosition::BottomLeft),
        "bottom-right" => Some(OverlayPosition::BottomRight),
        "center" => Some(OverlayPosition::Center),
        _ => None,
    }
}

fn overlay_position_str(v: OverlayPosition) -> &'static str {
    match v {
        OverlayPosition::Top => "top",
        OverlayPosition::Bottom => "bottom",
        OverlayPosition::TopLeft => "top-left",
        OverlayPosition::TopRight => "top-right",
        OverlayPosition::BottomLeft => "bottom-left",
        OverlayPosition::BottomRight => "bottom-right",
        OverlayPosition::Center => "center",
    }
}

fn parse_ec_level(s: &str) -> Option<EcLevel> {
    match s.to_ascii_uppercase().as_str() {
        "L" => Some(EcLevel::L),
        "M" => Some(EcLevel::M),
        "Q" => Some(EcLevel::Q),
        "H" => Some(EcLevel::H),
        _ => None,
    }
}

fn ec_level_str(v: EcLevel) -> &'static str {
    match v {
        EcLevel::L => "L",
        EcLevel::M => "M",
        EcLevel::Q => "Q",
        EcLevel::H => "H",
    }
}

fn parse_text_font(s: &str) -> Option<TextFont> {
    match s {
        "mono" => Some(TextFont::Mono),
        "mono-italic" => Some(TextFont::MonoItalic),
        _ => None,
    }
}

fn text_font_str(v: TextFont) -> &'static str {
    match v {
        TextFont::Mono => "mono",
        TextFont::MonoItalic => "mono-italic",
    }
}

fn base_scale(viewport: egui::Rect, bb: PolyRect) -> f32 {
    let w = bb.width().0.max(1) as f32;
    let h = bb.height().0.max(1) as f32;
    (viewport.width() / w).min(viewport.height() / h) * 0.95
}

fn fit_origin(viewport: egui::Rect, bb: PolyRect, scale: f32) -> Vec2 {
    let w = bb.width().0.max(1) as f32 * scale;
    let h = bb.height().0.max(1) as f32 * scale;
    Vec2::new(viewport.center().x - w * 0.5, viewport.center().y - h * 0.5)
}

fn to_screen_rect(r: PolyRect, bb: PolyRect, origin: Vec2, scale: f32) -> egui::Rect {
    let x0 = origin.x + (r.x0.0 - bb.x0.0) as f32 * scale;
    let x1 = origin.x + (r.x1.0 - bb.x0.0) as f32 * scale;
    let y0 = origin.y + (bb.y1.0 - r.y1.0) as f32 * scale;
    let y1 = origin.y + (bb.y1.0 - r.y0.0) as f32 * scale;
    egui::Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1, y1))
}

fn bitmap_box_to_layout_rect(
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    bitmap_h: u32,
    pitch_dbu: i32,
    pixel_w_dbu: i32,
) -> PolyRect {
    let x0 = x as i32 * pitch_dbu;
    let x1 = (x + w.saturating_sub(1)) as i32 * pitch_dbu + pixel_w_dbu;
    let y0 = (bitmap_h.saturating_sub(y + h)) as i32 * pitch_dbu;
    let y1 = (bitmap_h.saturating_sub(y + 1)) as i32 * pitch_dbu + pixel_w_dbu;
    PolyRect::new(x0, y0, x1, y1)
}

#[allow(clippy::too_many_arguments)]
fn padded_overlay_screen_rect(
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    padding: u32,
    bitmap_w: u32,
    bitmap_h: u32,
    pitch_dbu: i32,
    pixel_w_dbu: i32,
    bb: PolyRect,
    origin: Vec2,
    scale: f32,
) -> Option<egui::Rect> {
    let x0 = x.saturating_sub(padding);
    let y0 = y.saturating_sub(padding);
    let x1 = x.saturating_add(w).saturating_add(padding).min(bitmap_w);
    let y1 = y.saturating_add(h).saturating_add(padding).min(bitmap_h);
    let kw = x1.saturating_sub(x0);
    let kh = y1.saturating_sub(y0);
    if kw == 0 || kh == 0 {
        return None;
    }
    let kr = bitmap_box_to_layout_rect(x0, y0, kw, kh, bitmap_h, pitch_dbu, pixel_w_dbu);
    Some(to_screen_rect(kr, bb, origin, scale))
}

fn default_core_area_rect(full: PolyRect) -> PolyRect {
    let w = full.width().0.max(1);
    let h = full.height().0.max(1);
    if w < 4 || h < 4 {
        return full;
    }
    let mut inset_x = (w / DEFAULT_CORE_INSET_DIV).max(1);
    let mut inset_y = (h / DEFAULT_CORE_INSET_DIV).max(1);
    inset_x = inset_x.min((w / 2).saturating_sub(1));
    inset_y = inset_y.min((h / 2).saturating_sub(1));
    let x0 = full.x0.0 + inset_x;
    let y0 = full.y0.0 + inset_y;
    let x1 = full.x1.0 - inset_x;
    let y1 = full.y1.0 - inset_y;
    if x1 <= x0 || y1 <= y0 {
        full
    } else {
        PolyRect::new(x0, y0, x1, y1)
    }
}

fn overlay_position_combo(ui: &mut egui::Ui, id: &str, value: &mut OverlayPosition) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(match value {
            OverlayPosition::Top => "top",
            OverlayPosition::Bottom => "bottom",
            OverlayPosition::TopLeft => "top-left",
            OverlayPosition::TopRight => "top-right",
            OverlayPosition::BottomLeft => "bottom-left",
            OverlayPosition::BottomRight => "bottom-right",
            OverlayPosition::Center => "center",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(value, OverlayPosition::Top, "top");
            ui.selectable_value(value, OverlayPosition::Bottom, "bottom");
            ui.selectable_value(value, OverlayPosition::TopLeft, "top-left");
            ui.selectable_value(value, OverlayPosition::TopRight, "top-right");
            ui.selectable_value(value, OverlayPosition::BottomLeft, "bottom-left");
            ui.selectable_value(value, OverlayPosition::BottomRight, "bottom-right");
            ui.selectable_value(value, OverlayPosition::Center, "center");
        });
}

fn ec_level_combo(ui: &mut egui::Ui, id: &str, value: &mut EcLevel) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(match value {
            EcLevel::L => "L",
            EcLevel::M => "M",
            EcLevel::Q => "Q",
            EcLevel::H => "H",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(value, EcLevel::L, "L");
            ui.selectable_value(value, EcLevel::M, "M");
            ui.selectable_value(value, EcLevel::Q, "Q");
            ui.selectable_value(value, EcLevel::H, "H");
        });
}

fn image_resize_handle_rects(rect: egui::Rect, half_size: f32) -> [(ResizeCorner, egui::Rect); 4] {
    [
        (
            ResizeCorner::TopLeft,
            egui::Rect::from_center_size(rect.min, Vec2::splat(half_size * 2.0)),
        ),
        (
            ResizeCorner::TopRight,
            egui::Rect::from_center_size(
                Pos2::new(rect.max.x, rect.min.y),
                Vec2::splat(half_size * 2.0),
            ),
        ),
        (
            ResizeCorner::BottomLeft,
            egui::Rect::from_center_size(
                Pos2::new(rect.min.x, rect.max.y),
                Vec2::splat(half_size * 2.0),
            ),
        ),
        (
            ResizeCorner::BottomRight,
            egui::Rect::from_center_size(rect.max, Vec2::splat(half_size * 2.0)),
        ),
    ]
}

fn hit_test_resize_corner(
    p: Pos2,
    handles: &[(ResizeCorner, egui::Rect); 4],
) -> Option<ResizeCorner> {
    for (corner, rect) in handles {
        if rect.contains(p) {
            return Some(*corner);
        }
    }
    None
}

fn bitmap_to_color_image(bitmap: &ArtworkBitmap) -> egui::ColorImage {
    #[cfg(feature = "rayon")]
    {
        let mut rgba = vec![0u8; (bitmap.width * bitmap.height * 4) as usize];
        rgba.par_chunks_mut((bitmap.width * 4) as usize)
            .enumerate()
            .for_each(|(y, row)| {
                let y = y as u32;
                for x in 0..bitmap.width {
                    if bitmap.get(x, y) {
                        let i = (x * 4) as usize;
                        row[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
                    }
                }
            });
        egui::ColorImage::from_rgba_unmultiplied(
            [bitmap.width as usize, bitmap.height as usize],
            &rgba,
        )
    }

    #[cfg(not(feature = "rayon"))]
    {
        let mut rgba = Vec::with_capacity((bitmap.width * bitmap.height * 4) as usize);
        for y in 0..bitmap.height {
            for x in 0..bitmap.width {
                if bitmap.get(x, y) {
                    rgba.extend_from_slice(&[255, 255, 255, 255]);
                } else {
                    rgba.extend_from_slice(&[0, 0, 0, 0]);
                }
            }
        }
        return egui::ColorImage::from_rgba_unmultiplied(
            [bitmap.width as usize, bitmap.height as usize],
            &rgba,
        );
    }
}

fn build_rect_spatial_index(result: &GuiResult) -> RectSpatialIndex {
    let cell_dbu = (result.pitch_dbu.max(1) * 32).max(1);
    let mut map: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (idx, r) in result.rects.iter().enumerate() {
        let cx0 = r.x0.0.div_euclid(cell_dbu);
        let cx1 = r.x1.0.div_euclid(cell_dbu);
        let cy0 = r.y0.0.div_euclid(cell_dbu);
        let cy1 = r.y1.0.div_euclid(cell_dbu);
        for cx in cx0..=cx1 {
            for cy in cy0..=cy1 {
                map.entry((cx, cy)).or_default().push(idx);
            }
        }
    }
    RectSpatialIndex { cell_dbu, map }
}

fn find_hovered_rect(
    x: i32,
    y: i32,
    rects: &[PolyRect],
    index: Option<&RectSpatialIndex>,
) -> Option<usize> {
    if let Some(idx) = index {
        let cx = x.div_euclid(idx.cell_dbu);
        let cy = y.div_euclid(idx.cell_dbu);
        if let Some(candidates) = idx.map.get(&(cx, cy)) {
            for rid in candidates.iter().rev() {
                if rect_contains(rects[*rid], x, y) {
                    return Some(*rid);
                }
            }
            return None;
        }
    }
    for (rid, r) in rects.iter().enumerate().rev() {
        if rect_contains(*r, x, y) {
            return Some(rid);
        }
    }
    None
}

fn rect_contains(r: PolyRect, x: i32, y: i32) -> bool {
    x >= r.x0.0 && x <= r.x1.0 && y >= r.y0.0 && y <= r.y1.0
}

fn scaled_dim_from_pct(src_dim: u32, scale_pct: u32) -> u32 {
    ((src_dim as f64 * (scale_pct.max(1) as f64 / 100.0)).round() as u32).max(1)
}

fn rasterize_rects_to_image(result: &GuiResult) -> egui::ColorImage {
    let mut rgba = vec![0u8; (result.bitmap_w * result.bitmap_h * 4) as usize];
    for r in &result.rects {
        let x0 = (r.x0.0 / result.pitch_dbu).max(0) as u32;
        let x1 = ((r.x1.0 - result.pixel_w_dbu) / result.pitch_dbu).max(0) as u32;
        let n0 = (r.y0.0 / result.pitch_dbu).max(0);
        let n1 = ((r.y1.0 - result.pixel_w_dbu) / result.pitch_dbu).max(0);
        if x1 < x0 || n1 < n0 {
            continue;
        }
        let y = (result.bitmap_h as i32 - n1 - 1).max(0) as u32;
        let h = (n1 - n0 + 1).max(1) as u32;
        let w = (x1 - x0 + 1).max(1);
        let y_end = y.saturating_add(h).min(result.bitmap_h);
        let x_end = x0.saturating_add(w).min(result.bitmap_w);
        for py in y..y_end {
            let row = (py * result.bitmap_w * 4) as usize;
            for px in x0..x_end {
                let i = row + (px * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
    }
    egui::ColorImage::from_rgba_unmultiplied(
        [result.bitmap_w as usize, result.bitmap_h as usize],
        &rgba,
    )
}
