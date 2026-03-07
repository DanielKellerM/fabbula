// Copyright 2026 Daniel Keller <daniel.keller.m@gmail.com>
// Licensed under the Apache License, Version 2.0.
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::{Path, PathBuf};

use fabbula::artwork::{
    ArtworkBitmap, DitherMode, ThresholdMode, apply_exclusion_mask, load_artwork,
};
use fabbula::color::{LayerBitmap, extract_channels, extract_palette};
use fabbula::drc::{check_drc, report_drc};
use fabbula::gdsio::{LayerRects, merge_into_gds_multi, read_existing_metal, write_gds_multi};
use fabbula::generation::generate_layer_polygons;
use fabbula::lef::{LefLayer, write_lef_multi};
use fabbula::pdk::{ArtworkLayerProfile, DrcRules, PdkConfig};
use fabbula::polygon::{PixelPlacement, PolygonStrategy, Rect, bounding_box};
use fabbula::preview::{
    DEFAULT_LAYER_COLORS, HtmlLayer, SvgLayer, write_deep_zoom_preview, write_html_preview_multi,
    write_svg_multi,
};
use fabbula::tiles::{TileConfig, TileLayer, generate_tile_pyramid, parse_hex_color};

#[derive(Parser)]
#[command(
    name = "fabbula",
    about = "Multi-PDK, DRC-aware image-to-GDSII artwork generator",
    version,
    long_about = r#"
fabbula converts images (PNG, SVG, etc.) into GDSII layout data suitable
for embedding as artwork on top-metal layers of integrated circuits.

Unlike other tools, fabbula:
  • 6 built-in PDKs (SKY130, IHP SG13G2, GF180MCU, FreePDK45, ASAP7, fabbula2) + custom TOML
  • Generates DRC-clean output by construction
  • Uses efficient polygon merging strategies
  • Is written in Rust for speed and correctness

EXAMPLES:
  # Basic: convert logo to GDS for SKY130
  fabbula generate -i logo.png -o logo.gds -p sky130

  # With custom PDK and SVG preview
  fabbula generate -i logo.png -o logo.gds -p my_pdk.toml --svg preview.svg

  # Merge artwork into existing chip GDS
  fabbula merge -i logo.png --chip chip.gds -o chip_with_art.gds -p ihp

  # List available built-in PDKs
  fabbula list-pdks
"#
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Convert an image to a standalone GDSII artwork file
    Generate {
        /// Input image (PNG, JPEG, BMP, etc.)
        #[arg(short, long)]
        input: PathBuf,

        /// Output GDSII file
        #[arg(short, long)]
        output: PathBuf,

        /// PDK name (sky130, ihp_sg13g2, gf180mcu) or path to custom .toml
        #[arg(short, long)]
        pdk: String,

        /// Cell name in the output GDS
        #[arg(long, default_value = "artwork")]
        cell_name: String,

        /// Threshold for converting image to binary (0-255, or "otsu" for automatic)
        #[arg(long, default_value = "128")]
        threshold: String,

        /// Polygon merging strategy
        #[arg(long, default_value = "greedy-merge", value_enum)]
        strategy: StrategyArg,

        /// Use separated mode with guaranteed spacing between all pixels
        #[arg(long)]
        separated: bool,

        /// Maximum image width in pixels (for limiting GDS complexity)
        #[arg(long)]
        max_width: Option<u32>,

        /// Maximum image height in pixels
        #[arg(long)]
        max_height: Option<u32>,

        /// Physical artwork size in micrometers (e.g. "2000x2000" for 2mm x 2mm).
        /// Computes max_width/max_height from PDK pitch automatically.
        #[arg(long)]
        size_um: Option<String>,

        /// Invert the image (swap metal/gap)
        #[arg(long)]
        invert: bool,

        /// Output SVG preview file
        #[arg(long)]
        svg: Option<PathBuf>,

        /// Output interactive HTML preview file
        #[arg(long)]
        html: Option<PathBuf>,

        /// Output LEF macro file (for OpenLane/OpenROAD integration)
        #[arg(long)]
        lef: Option<PathBuf>,

        /// Run DRC check on output
        #[arg(long)]
        check_drc: bool,

        /// Disable automatic density enforcement (allow density violations)
        #[arg(long)]
        no_density_enforce: bool,

        /// Apply Floyd-Steinberg dithering (improves gradients at chip scale)
        #[arg(long)]
        dither: bool,

        /// Color extraction mode for multi-layer output
        #[arg(long, default_value = "single", value_enum)]
        color_mode: ColorModeArg,

        /// Number of palette colors (for palette mode; defaults to number of artwork layers)
        #[arg(long)]
        num_colors: Option<usize>,

        /// Generate deep zoom tile pyramid for HTML preview (requires --html)
        #[arg(long)]
        deep_zoom: bool,

        /// Max resolution for deep zoom tile pyramid in pixels (default 4096)
        #[arg(long, default_value = "4096")]
        tile_resolution: u32,
    },

    /// Merge artwork into an existing GDSII chip file
    Merge {
        /// Input image
        #[arg(short, long)]
        input: PathBuf,

        /// Input chip GDSII file
        #[arg(long)]
        chip: PathBuf,

        /// Output GDSII file (chip + artwork)
        #[arg(short, long)]
        output: PathBuf,

        /// PDK name or path
        #[arg(short, long)]
        pdk: String,

        /// Target cell in the chip GDS (default: top cell)
        #[arg(long)]
        cell: Option<String>,

        /// X offset in µm for artwork placement
        #[arg(long, default_value = "0.0")]
        offset_x: f64,

        /// Y offset in µm for artwork placement
        #[arg(long, default_value = "0.0")]
        offset_y: f64,

        /// Threshold
        #[arg(long, default_value = "128")]
        threshold: String,

        /// Strategy
        #[arg(long, default_value = "greedy-merge", value_enum)]
        strategy: StrategyArg,

        /// Use separated mode with guaranteed spacing between all pixels
        #[arg(long)]
        separated: bool,

        /// Max width
        #[arg(long)]
        max_width: Option<u32>,

        /// Max height
        #[arg(long)]
        max_height: Option<u32>,

        /// Physical artwork size in micrometers (e.g. "2000x2000" for 2mm x 2mm)
        #[arg(long)]
        size_um: Option<String>,

        /// Invert
        #[arg(long)]
        invert: bool,

        /// Exclusion margin in um - clear artwork pixels near existing metal
        #[arg(long)]
        exclusion_margin: Option<f64>,

        /// GDS layer/datatype for exclusion (e.g. "81/0"). Defaults to PDK artwork layer.
        #[arg(long)]
        exclusion_layer: Option<String>,

        /// Apply Floyd-Steinberg dithering (improves gradients at chip scale)
        #[arg(long)]
        dither: bool,

        /// Disable automatic density enforcement
        #[arg(long)]
        no_density_enforce: bool,

        /// Color extraction mode for multi-layer output
        #[arg(long, default_value = "single", value_enum)]
        color_mode: ColorModeArg,

        /// Number of palette colors (for palette mode)
        #[arg(long)]
        num_colors: Option<usize>,
    },

    /// List available built-in PDK configurations
    ListPdks,

    /// Show PDK details
    ShowPdk {
        /// PDK name or path
        pdk: String,
    },
}

#[derive(Debug, Clone, ValueEnum)]
enum StrategyArg {
    PixelRects,
    RowMerge,
    GreedyMerge,
    HistogramMerge,
}

#[derive(Debug, Clone, ValueEnum)]
enum ColorModeArg {
    /// Single-layer (default, existing behavior)
    Single,
    /// Map R/G/B channels to separate layers
    Channel,
    /// K-means color quantization into N layers
    Palette,
}

impl From<StrategyArg> for PolygonStrategy {
    fn from(s: StrategyArg) -> Self {
        match s {
            StrategyArg::PixelRects => PolygonStrategy::PixelRects,
            StrategyArg::RowMerge => PolygonStrategy::RowMerge,
            StrategyArg::GreedyMerge => PolygonStrategy::GreedyMerge,
            StrategyArg::HistogramMerge => PolygonStrategy::HistogramMerge,
        }
    }
}

fn most_conservative_drc(profiles: &[ArtworkLayerProfile]) -> DrcRules {
    let rules: Vec<_> = profiles.iter().map(|p| p.drc.clone()).collect();
    DrcRules::most_conservative(&rules)
}

fn load_pdk(name_or_path: &str) -> Result<PdkConfig> {
    if name_or_path.ends_with(".toml") {
        PdkConfig::from_file(Path::new(name_or_path))
    } else {
        PdkConfig::builtin(name_or_path)
    }
}

fn parse_threshold(s: &str) -> Result<ThresholdMode> {
    if s.eq_ignore_ascii_case("otsu") {
        Ok(ThresholdMode::Otsu)
    } else if s.eq_ignore_ascii_case("alpha") {
        Ok(ThresholdMode::Alpha(128))
    } else if let Ok(v) = s.parse::<u8>() {
        Ok(ThresholdMode::Luminance(v))
    } else {
        anyhow::bail!(
            "Invalid threshold '{}': expected 'otsu', 'alpha', or a number 0-255",
            s
        )
    }
}

/// Parse a "WxH" size string in micrometers and convert to pixel dimensions using PDK pitch.
fn parse_size_um(s: &str, pdk: &PdkConfig, drc: &DrcRules, touching: bool) -> Result<(u32, u32)> {
    let (w_str, h_str) = s
        .split_once('x')
        .ok_or_else(|| anyhow::anyhow!("size-um must be in WxH format (e.g. 2000x2000)"))?;
    let w_um: f64 = w_str
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid width in size-um"))?;
    let h_um: f64 = h_str
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid height in size-um"))?;

    let min_w_um = pdk.snap_to_grid(drc.min_width);
    let eff_s_um = pdk.snap_to_grid(drc.effective_spacing());
    let pitch_um = if touching {
        min_w_um
    } else {
        min_w_um + eff_s_um
    };

    let px_w = (w_um / pitch_um).floor() as u32;
    let px_h = (h_um / pitch_um).floor() as u32;
    anyhow::ensure!(
        px_w > 0 && px_h > 0,
        "size-um too small for PDK pitch ({}um)",
        pitch_um
    );

    tracing::info!(
        "size-um: {:.1}x{:.1} um -> {}x{} pixels (pitch={:.3} um)",
        w_um,
        h_um,
        px_w,
        px_h,
        pitch_um
    );
    Ok((px_w, px_h))
}

fn prepare_bitmap(
    input: &Path,
    threshold: &str,
    max_width: Option<u32>,
    max_height: Option<u32>,
    invert: bool,
    dither: DitherMode,
) -> Result<ArtworkBitmap> {
    let thresh = parse_threshold(threshold)?;
    let max_px = match (max_width, max_height) {
        (Some(w), Some(h)) => Some((w, h)),
        (Some(w), None) => Some((w, w)),
        (None, Some(h)) => Some((h, h)),
        (None, None) => None,
    };
    let mut bitmap = load_artwork(input, thresh, max_px, dither)?;
    if invert {
        bitmap.invert();
    }
    Ok(bitmap)
}

/// Parse a "LAYER/DATATYPE" string into (i16, i16).
fn parse_layer_spec(s: &str) -> Result<(i16, i16)> {
    let (layer_str, dt_str) = s.split_once('/').ok_or_else(|| {
        anyhow::anyhow!("exclusion-layer must be in LAYER/DATATYPE format (e.g. 81/0)")
    })?;
    let layer: i16 = layer_str
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid layer number in exclusion-layer"))?;
    let datatype: i16 = dt_str
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid datatype in exclusion-layer"))?;
    Ok((layer, datatype))
}

fn report_bounds(layer_results: &[(Vec<Rect>, &ArtworkLayerProfile)], pdk: &PdkConfig) {
    let all_rects: Vec<Rect> = layer_results
        .iter()
        .flat_map(|(r, _)| r.iter().copied())
        .collect();
    if let Some(bb) = bounding_box(&all_rects) {
        let dbu_per_um = pdk.pdk.db_units_per_um as f64;
        let w_um = bb.width() as f64 / dbu_per_um;
        let h_um = bb.height() as f64 / dbu_per_um;
        tracing::info!(
            "Artwork bounds: ({:.2}, {:.2}) to ({:.2}, {:.2}) um - {:.1} x {:.1} um ({:.3} x {:.3} mm)",
            bb.x0 as f64 / dbu_per_um,
            bb.y0 as f64 / dbu_per_um,
            bb.x1 as f64 / dbu_per_um,
            bb.y1 as f64 / dbu_per_um,
            w_um,
            h_um,
            w_um / 1000.0,
            h_um / 1000.0
        );
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "fabbula=info".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Generate {
            input,
            output,
            pdk,
            cell_name,
            threshold,
            strategy,
            separated,
            max_width,
            max_height,
            size_um,
            invert,
            svg,
            html,
            lef,
            check_drc: do_drc,
            no_density_enforce,
            dither,
            color_mode,
            num_colors,
            deep_zoom,
            tile_resolution,
        } => {
            let pdk = load_pdk(&pdk)?;
            let strategy: PolygonStrategy = strategy.into();
            let placement = if separated {
                PixelPlacement::Separated
            } else {
                PixelPlacement::Touching
            };
            let dither_mode = if dither {
                DitherMode::FloydSteinberg
            } else {
                DitherMode::Off
            };
            let density_enforce = !no_density_enforce;
            let profiles = pdk.layer_profiles();
            // For size_um, use the DRC rules that will actually be used for generation
            let size_um_drc = match color_mode {
                ColorModeArg::Single => profiles[0].drc.clone(),
                ColorModeArg::Channel | ColorModeArg::Palette => most_conservative_drc(&profiles),
            };
            let (max_width, max_height) = if let Some(ref size_str) = size_um {
                let (pw, ph) =
                    parse_size_um(size_str, &pdk, &size_um_drc, placement.is_touching())?;
                (
                    Some(max_width.unwrap_or(pw).min(pw)),
                    Some(max_height.unwrap_or(ph).min(ph)),
                )
            } else {
                (max_width, max_height)
            };
            let max_px = match (max_width, max_height) {
                (Some(w), Some(h)) => Some((w, h)),
                (Some(w), None) => Some((w, w)),
                (None, Some(h)) => Some((h, h)),
                (None, None) => None,
            };
            let thresh = parse_threshold(&threshold)?;

            // Collect per-layer rects and profile references
            let mut layer_results: Vec<(Vec<Rect>, &ArtworkLayerProfile)> = Vec::new();

            match color_mode {
                ColorModeArg::Single => {
                    let mut bitmap = prepare_bitmap(
                        &input,
                        &threshold,
                        max_width,
                        max_height,
                        invert,
                        dither_mode,
                    )?;
                    tracing::info!(
                        "Bitmap: {}x{}, density: {:.1}%",
                        bitmap.width,
                        bitmap.height,
                        bitmap.density() * 100.0
                    );
                    let profile = &profiles[0];
                    let rects = generate_layer_polygons(
                        &mut bitmap,
                        &pdk,
                        &profile.drc,
                        strategy,
                        placement,
                        density_enforce,
                    )?;
                    layer_results.push((rects, profile));
                }
                ColorModeArg::Channel => {
                    // Use the most conservative pitch so all layers align spatially
                    let shared_drc = most_conservative_drc(&profiles);
                    let layer_bitmaps = extract_channels(&input, &profiles, thresh, max_px)?;
                    for LayerBitmap {
                        mut bitmap,
                        layer_index,
                    } in layer_bitmaps
                    {
                        if invert {
                            bitmap.invert();
                        }
                        let profile = &profiles[layer_index];
                        let rects = generate_layer_polygons(
                            &mut bitmap,
                            &pdk,
                            &shared_drc,
                            strategy,
                            placement,
                            density_enforce,
                        )?;
                        layer_results.push((rects, profile));
                    }
                }
                ColorModeArg::Palette => {
                    // Use the most conservative pitch so all layers align spatially
                    let shared_drc = most_conservative_drc(&profiles);
                    let n = num_colors.unwrap_or(profiles.len());
                    anyhow::ensure!(
                        n <= profiles.len(),
                        "num_colors ({}) exceeds available artwork layer profiles ({}); \
                         add more [[artwork_layers]] to your PDK or reduce --num-colors",
                        n,
                        profiles.len()
                    );
                    let layer_bitmaps = extract_palette(&input, n, max_px)?;
                    for LayerBitmap {
                        mut bitmap,
                        layer_index,
                    } in layer_bitmaps
                    {
                        if invert {
                            bitmap.invert();
                        }
                        let profile = &profiles[layer_index];
                        let rects = generate_layer_polygons(
                            &mut bitmap,
                            &pdk,
                            &shared_drc,
                            strategy,
                            placement,
                            density_enforce,
                        )?;
                        layer_results.push((rects, profile));
                    }
                }
            }

            // Report artwork bounds
            report_bounds(&layer_results, &pdk);

            // DRC check per layer (uses each layer's own rules)
            if do_drc {
                for (rects, profile) in &layer_results {
                    tracing::info!("DRC check for layer '{}':", profile.name);
                    let violations = check_drc(rects, pdk.pdk.db_units_per_um, &profile.drc);
                    report_drc(&violations);
                }
            }

            // Write GDS
            let gds_layers: Vec<LayerRects> = layer_results
                .iter()
                .map(|(rects, profile)| LayerRects {
                    rects,
                    layer: profile.gds_layer,
                    datatype: profile.gds_datatype,
                })
                .collect();
            write_gds_multi(&gds_layers, &cell_name, &output)?;

            // LEF
            if let Some(lef_path) = lef {
                let lef_layers: Vec<LefLayer> = layer_results
                    .iter()
                    .map(|(rects, profile)| LefLayer {
                        rects,
                        layer_name: &profile.name,
                    })
                    .collect();
                write_lef_multi(&lef_layers, &pdk, &cell_name, &lef_path)?;
            }

            // SVG
            if let Some(svg_path) = svg {
                let svg_layers: Vec<SvgLayer> = layer_results
                    .iter()
                    .enumerate()
                    .map(|(i, (rects, _))| SvgLayer {
                        rects: rects.as_slice(),
                        color: DEFAULT_LAYER_COLORS[i % DEFAULT_LAYER_COLORS.len()],
                    })
                    .collect();
                write_svg_multi(&svg_layers, &svg_path, 0.01, Some("#1a1a2e"))?;
            }

            // HTML
            if let Some(html_path) = html {
                let html_layers: Vec<HtmlLayer> = layer_results
                    .iter()
                    .enumerate()
                    .map(|(i, (rects, profile))| HtmlLayer {
                        rects,
                        name: &profile.name,
                        color: DEFAULT_LAYER_COLORS[i % DEFAULT_LAYER_COLORS.len()],
                    })
                    .collect();

                if deep_zoom {
                    // Generate tile pyramid
                    let tile_dir = html_path.with_extension("").with_file_name(format!(
                        "{}_tiles",
                        html_path.file_stem().unwrap_or_default().to_string_lossy()
                    ));
                    // Place tile_dir next to html file
                    let tile_dir = html_path.parent().unwrap_or(Path::new(".")).join(
                        tile_dir
                            .file_name()
                            .expect("tile_dir has a filename component"),
                    );
                    std::fs::create_dir_all(&tile_dir)?;

                    let bb = bounding_box(
                        &layer_results
                            .iter()
                            .flat_map(|(r, _)| r.iter().copied())
                            .collect::<Vec<_>>(),
                    )
                    .unwrap_or(Rect::new(0, 0, 1000, 1000));

                    let tile_layers: Vec<TileLayer> = layer_results
                        .iter()
                        .enumerate()
                        .map(|(i, (rects, profile))| TileLayer {
                            rects,
                            color: parse_hex_color(
                                DEFAULT_LAYER_COLORS[i % DEFAULT_LAYER_COLORS.len()],
                            ),
                            name: &profile.name,
                        })
                        .collect();

                    let config = TileConfig {
                        tile_size: 256,
                        max_resolution: tile_resolution,
                    };
                    generate_tile_pyramid(&tile_layers, &bb, &config, &tile_dir)?;
                    write_deep_zoom_preview(&html_layers, &html_path, &pdk, &tile_dir)?;
                } else {
                    write_html_preview_multi(&html_layers, &html_path, &pdk)?;
                }
            }

            tracing::info!("Done! Output: {}", output.display());
        }

        Commands::Merge {
            input,
            chip,
            output,
            pdk,
            cell,
            offset_x,
            offset_y,
            threshold,
            strategy,
            separated,
            max_width,
            max_height,
            size_um,
            invert,
            exclusion_margin,
            exclusion_layer,
            dither,
            no_density_enforce,
            color_mode,
            num_colors,
        } => {
            let pdk = load_pdk(&pdk)?;
            let strategy: PolygonStrategy = strategy.into();
            let placement = if separated {
                PixelPlacement::Separated
            } else {
                PixelPlacement::Touching
            };
            let dither_mode = if dither {
                DitherMode::FloydSteinberg
            } else {
                DitherMode::Off
            };
            let density_enforce = !no_density_enforce;
            let profiles = pdk.layer_profiles();
            let size_um_drc = match color_mode {
                ColorModeArg::Single => profiles[0].drc.clone(),
                ColorModeArg::Channel | ColorModeArg::Palette => most_conservative_drc(&profiles),
            };
            let (max_width, max_height) = if let Some(ref size_str) = size_um {
                let (pw, ph) =
                    parse_size_um(size_str, &pdk, &size_um_drc, placement.is_touching())?;
                (
                    Some(max_width.unwrap_or(pw).min(pw)),
                    Some(max_height.unwrap_or(ph).min(ph)),
                )
            } else {
                (max_width, max_height)
            };
            let max_px = match (max_width, max_height) {
                (Some(w), Some(h)) => Some((w, h)),
                (Some(w), None) => Some((w, w)),
                (None, Some(h)) => Some((h, h)),
                (None, None) => None,
            };
            let thresh = parse_threshold(&threshold)?;

            let mut layer_results: Vec<(Vec<Rect>, &ArtworkLayerProfile)> = Vec::new();

            // Read existing metal once for exclusion (shared across all color modes)
            let exclusion_override = exclusion_layer
                .as_deref()
                .map(parse_layer_spec)
                .transpose()?;
            let exclusion_metal = if exclusion_margin.is_some() {
                let existing =
                    read_existing_metal(&chip, &pdk, cell.as_deref(), exclusion_override)?;
                if existing.is_empty() {
                    None
                } else {
                    Some(existing)
                }
            } else {
                None
            };

            match color_mode {
                ColorModeArg::Single => {
                    let mut bitmap = prepare_bitmap(
                        &input,
                        &threshold,
                        max_width,
                        max_height,
                        invert,
                        dither_mode,
                    )?;

                    if let (Some(margin_um), Some(existing)) = (exclusion_margin, &exclusion_metal)
                    {
                        let margin_dbu = pdk.um_to_dbu(margin_um);
                        apply_exclusion_mask(&mut bitmap, existing, &pdk, margin_dbu);
                    }

                    let profile = &profiles[0];
                    let rects = generate_layer_polygons(
                        &mut bitmap,
                        &pdk,
                        &profile.drc,
                        strategy,
                        placement,
                        density_enforce,
                    )?;
                    layer_results.push((rects, profile));
                }
                ColorModeArg::Channel => {
                    let shared_drc = most_conservative_drc(&profiles);
                    let layer_bitmaps = extract_channels(&input, &profiles, thresh, max_px)?;
                    for LayerBitmap {
                        mut bitmap,
                        layer_index,
                    } in layer_bitmaps
                    {
                        if invert {
                            bitmap.invert();
                        }
                        if let (Some(margin_um), Some(existing)) =
                            (exclusion_margin, &exclusion_metal)
                        {
                            let margin_dbu = pdk.um_to_dbu(margin_um);
                            apply_exclusion_mask(&mut bitmap, existing, &pdk, margin_dbu);
                        }
                        let profile = &profiles[layer_index];
                        let rects = generate_layer_polygons(
                            &mut bitmap,
                            &pdk,
                            &shared_drc,
                            strategy,
                            placement,
                            density_enforce,
                        )?;
                        layer_results.push((rects, profile));
                    }
                }
                ColorModeArg::Palette => {
                    let shared_drc = most_conservative_drc(&profiles);
                    let n = num_colors.unwrap_or(profiles.len());
                    anyhow::ensure!(
                        n <= profiles.len(),
                        "num_colors ({}) exceeds available artwork layer profiles ({}); \
                         add more [[artwork_layers]] to your PDK or reduce --num-colors",
                        n,
                        profiles.len()
                    );
                    let layer_bitmaps = extract_palette(&input, n, max_px)?;
                    for LayerBitmap {
                        mut bitmap,
                        layer_index,
                    } in layer_bitmaps
                    {
                        if invert {
                            bitmap.invert();
                        }
                        if let (Some(margin_um), Some(existing)) =
                            (exclusion_margin, &exclusion_metal)
                        {
                            let margin_dbu = pdk.um_to_dbu(margin_um);
                            apply_exclusion_mask(&mut bitmap, existing, &pdk, margin_dbu);
                        }
                        let profile = &profiles[layer_index];
                        let rects = generate_layer_polygons(
                            &mut bitmap,
                            &pdk,
                            &shared_drc,
                            strategy,
                            placement,
                            density_enforce,
                        )?;
                        layer_results.push((rects, profile));
                    }
                }
            }

            // Report artwork bounds
            report_bounds(&layer_results, &pdk);

            let ox = pdk.um_to_dbu(offset_x);
            let oy = pdk.um_to_dbu(offset_y);

            let gds_layers: Vec<LayerRects> = layer_results
                .iter()
                .map(|(rects, profile)| LayerRects {
                    rects,
                    layer: profile.gds_layer,
                    datatype: profile.gds_datatype,
                })
                .collect();
            merge_into_gds_multi(&gds_layers, &chip, &output, cell.as_deref(), ox, oy)?;

            // Report placed bounds (with offset)
            let offset_rects: Vec<Rect> = layer_results
                .iter()
                .flat_map(|(r, _)| {
                    r.iter().map(|rect| {
                        Rect::new(rect.x0 + ox, rect.y0 + oy, rect.x1 + ox, rect.y1 + oy)
                    })
                })
                .collect();
            if let Some(bb) = bounding_box(&offset_rects) {
                let dbu_per_um = pdk.pdk.db_units_per_um as f64;
                tracing::info!(
                    "Placed artwork bounds: ({:.2}, {:.2}) to ({:.2}, {:.2}) um",
                    bb.x0 as f64 / dbu_per_um,
                    bb.y0 as f64 / dbu_per_um,
                    bb.x1 as f64 / dbu_per_um,
                    bb.y1 as f64 / dbu_per_um
                );
            }

            tracing::info!("Done! Merged artwork into: {}", output.display());
        }

        Commands::ListPdks => {
            println!("Built-in PDK configurations:");
            for builtin in PdkConfig::list_builtins() {
                let name = builtin.name();
                let Ok(pdk) = PdkConfig::builtin(name) else {
                    println!("  {:<15} (failed to load)", name);
                    continue;
                };
                println!(
                    "  {:<15} {}  (artwork layer: {}/{})",
                    name,
                    pdk.pdk.description,
                    pdk.artwork_layer.gds_layer,
                    pdk.artwork_layer.gds_datatype
                );
            }
            println!("\nYou can also provide a custom .toml file with -p path/to/pdk.toml");
        }

        Commands::ShowPdk { pdk } => {
            let config = load_pdk(&pdk)?;
            println!("PDK: {} ({})", config.pdk.name, config.pdk.description);
            println!("Node: {} nm", config.pdk.node_nm);
            println!("DB units/µm: {}", config.pdk.db_units_per_um);
            println!();
            println!(
                "Artwork layer: {} (GDS {}/{})",
                config.artwork_layer.name,
                config.artwork_layer.gds_layer,
                config.artwork_layer.gds_datatype
            );
            println!();
            println!("DRC rules:");
            println!("  Min width:   {} µm", config.drc.min_width);
            println!("  Min spacing: {} µm", config.drc.min_spacing);
            println!("  Min area:    {} µm²", config.drc.min_area);
            if let Some(max_w) = config.drc.max_width {
                println!("  Max width:   {} µm (slotting)", max_w);
            }
            println!("  Max density: {:.0}%", config.drc.density_max * 100.0);
            println!();
            println!("Grid: {} µm", config.grid.manufacturing_grid_um);
            println!(
                "Pixel pitch: {:.3} µm (DRC-safe pixel size)",
                config.pixel_pitch_um()
            );
            println!();
            println!("Metal stack:");
            for m in &config.metal_stack {
                println!("  {} (GDS {}/{})", m.name, m.gds_layer, m.gds_datatype);
            }
        }
    }

    Ok(())
}
