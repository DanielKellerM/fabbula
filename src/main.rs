use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::{Path, PathBuf};

use fabbula::artwork::{apply_exclusion_mask, load_artwork, ArtworkBitmap, ThresholdMode};
use fabbula::drc::{check_drc, report_drc};
use fabbula::gdsio::{merge_into_gds, read_existing_metal, write_gds};
use fabbula::lef::write_lef;
use fabbula::pdk::PdkConfig;
use fabbula::polygon::{generate_polygons, PolygonStrategy};
use fabbula::preview::{write_html_preview, write_svg};

#[derive(Parser)]
#[command(
    name = "fabbula",
    about = "Multi-PDK, DRC-aware image-to-GDSII artwork generator",
    version,
    long_about = r#"
fabbula converts images (PNG, SVG, etc.) into GDSII layout data suitable
for embedding as artwork on top-metal layers of integrated circuits.

Unlike other tools, fabbula:
  • Supports multiple open PDKs (SKY130, IHP SG13G2, GF180MCU)
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

        /// Allow adjacent metal pixels to touch (denser artwork, fewer DRC guarantees)
        #[arg(long)]
        touching: bool,

        /// Maximum image width in pixels (for limiting GDS complexity)
        #[arg(long)]
        max_width: Option<u32>,

        /// Maximum image height in pixels
        #[arg(long)]
        max_height: Option<u32>,

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

        /// Allow touching
        #[arg(long)]
        touching: bool,

        /// Max width
        #[arg(long)]
        max_width: Option<u32>,

        /// Max height
        #[arg(long)]
        max_height: Option<u32>,

        /// Invert
        #[arg(long)]
        invert: bool,

        /// Exclusion margin in um - clear artwork pixels near existing metal
        #[arg(long)]
        exclusion_margin: Option<f64>,
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
}

impl From<StrategyArg> for PolygonStrategy {
    fn from(s: StrategyArg) -> Self {
        match s {
            StrategyArg::PixelRects => PolygonStrategy::PixelRects,
            StrategyArg::RowMerge => PolygonStrategy::RowMerge,
            StrategyArg::GreedyMerge => PolygonStrategy::GreedyMerge,
        }
    }
}

fn load_pdk(name_or_path: &str) -> Result<PdkConfig> {
    if name_or_path.ends_with(".toml") {
        PdkConfig::from_file(Path::new(name_or_path))
    } else {
        PdkConfig::builtin(name_or_path)
    }
}

fn parse_threshold(s: &str) -> ThresholdMode {
    if s.eq_ignore_ascii_case("otsu") {
        ThresholdMode::Otsu
    } else if s.eq_ignore_ascii_case("alpha") {
        ThresholdMode::Alpha(128)
    } else if let Ok(v) = s.parse::<u8>() {
        ThresholdMode::Luminance(v)
    } else {
        ThresholdMode::Luminance(128)
    }
}

fn prepare_bitmap(
    input: &Path,
    threshold: &str,
    max_width: Option<u32>,
    max_height: Option<u32>,
    invert: bool,
) -> Result<ArtworkBitmap> {
    let thresh = parse_threshold(threshold);
    let max_px = match (max_width, max_height) {
        (Some(w), Some(h)) => Some((w, h)),
        (Some(w), None) => Some((w, w)),
        (None, Some(h)) => Some((h, h)),
        (None, None) => None,
    };
    let mut bitmap = load_artwork(input, thresh, max_px)?;
    if invert {
        bitmap.invert();
    }
    Ok(bitmap)
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
            touching,
            max_width,
            max_height,
            invert,
            svg,
            html,
            lef,
            check_drc: do_drc,
        } => {
            let pdk = load_pdk(&pdk)?;
            let bitmap = prepare_bitmap(&input, &threshold, max_width, max_height, invert)?;

            tracing::info!(
                "Bitmap: {}x{}, density: {:.1}%",
                bitmap.width,
                bitmap.height,
                bitmap.density() * 100.0
            );

            let rects = generate_polygons(&bitmap, &pdk, strategy.into(), touching)?;

            if do_drc {
                let drc_rules = pdk.active_drc(false);
                let violations = check_drc(&rects, pdk.pdk.db_units_per_um, drc_rules);
                report_drc(&violations);
            }

            write_gds(&rects, &pdk, &cell_name, &output)?;

            if let Some(lef_path) = lef {
                write_lef(&rects, &pdk, &cell_name, &lef_path)?;
            }

            if let Some(svg_path) = svg {
                write_svg(&rects, &svg_path, 0.01, "#c0c0c0", Some("#1a1a2e"))?;
            }

            if let Some(html_path) = html {
                write_html_preview(&rects, &html_path, &pdk)?;
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
            touching,
            max_width,
            max_height,
            invert,
            exclusion_margin,
        } => {
            let pdk = load_pdk(&pdk)?;
            let mut bitmap = prepare_bitmap(&input, &threshold, max_width, max_height, invert)?;

            // Apply exclusion zones from existing metal in the chip GDS
            if let Some(margin_um) = exclusion_margin {
                let existing = read_existing_metal(&chip, &pdk, cell.as_deref())?;
                if !existing.is_empty() {
                    let margin_dbu = pdk.um_to_dbu(margin_um);
                    apply_exclusion_mask(&mut bitmap, &existing, &pdk, margin_dbu);
                }
            }

            let rects = generate_polygons(&bitmap, &pdk, strategy.into(), touching)?;

            let ox = pdk.um_to_dbu(offset_x);
            let oy = pdk.um_to_dbu(offset_y);

            merge_into_gds(&rects, &pdk, &chip, &output, cell.as_deref(), ox, oy)?;

            tracing::info!("Done! Merged artwork into: {}", output.display());
        }

        Commands::ListPdks => {
            println!("Built-in PDK configurations:");
            for name in PdkConfig::list_builtins() {
                let pdk = PdkConfig::builtin(name).unwrap();
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
            println!("  Max density: {:.0}%", config.drc.density_max * 100.0);
            println!();
            println!("Grid: {} µm", config.grid.manufacturing_grid_um);
            println!(
                "Pixel pitch: {} µm (DRC-safe pixel size)",
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
