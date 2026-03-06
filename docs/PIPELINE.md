# fabbula: The Full Pipeline

## AI Art → SVG → DRC-Clean GDSII

```
┌─────────────────┐    ┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│  1. AI IMAGE     │───▶│ 2. VECTORIZE │───▶│ 3. SVG→POLY  │───▶│ 4. POLY→GDS  │
│  (ChatGPT/Gemini)│    │ (vtracer)    │    │ (usvg+lyon)  │    │ (gds21)      │
└─────────────────┘    └──────────────┘    └──────────────┘    └──────────────┘
       PNG/WebP              SVG              Polygons            GDSII
                                              snapped to
                                              DRC grid
```

---

## Stage 1: AI Image Generation - Prompt Engineering

The key insight: **chip artwork lives on a single metal layer**, which means it's
fundamentally **monochrome** - the metal is either there or it isn't. Your AI prompt
needs to produce images that vectorize cleanly into solid black/white regions.

### What makes a good chip art source image

- **High contrast** - pure black on white (or vice versa)
- **No gradients** - hard edges, solid fills
- **No anti-aliasing artifacts** - the cleaner the edge, the fewer polygons
- **Bold shapes** - fine detail below ~2µm will be lost (that's the minimum feature size)
- **No text at small sizes** - text needs to be large enough to survive DRC rules
- **Square or chip-shaped** aspect ratio

### Prompt templates

#### For ChatGPT (DALL-E 3):

```
Create a black and white logo/emblem/mascot for a silicon chip.
The design should be:
- Pure black shapes on a pure white background
- NO gradients, NO gray tones, NO anti-aliasing
- Bold, clean lines with minimum detail size of 2% of the image width
- Style: [woodcut / linocut / stencil / stamp / pixel art]
- Subject: [your subject: e.g. "a meerkat", "university crest", "a rocket"]
- The image should look good as a physical metal etching
- Aspect ratio: 1:1
- Resolution: 1024x1024
```

**Pro variant (for more control):**

```
Design a high-contrast stencil art suitable for metal etching on a
microchip die. The art will be fabricated at ~130nm process, so the
smallest features should be at least 2% of the total image width.

Requirements:
- ONLY pure black (#000000) and pure white (#FFFFFF)
- No gradients, dithering, halftones, or gray values
- Clean vector-like edges (as if cut from a single sheet)
- No isolated pixels or thin bridges between shapes
- Subject: [description]
- Style: bold geometric / minimal line art / tribal / art deco
- Must work as a single connected stencil (no floating islands
  unless intentional)
```

#### For Gemini (Imagen 3):

```
Generate a black and white stencil design of [subject].
Pure black on white, no gradients.
Style: linocut print / woodblock / stencil art.
Bold clean shapes, suitable for laser cutting or metal etching.
Square format.
```

#### Style keywords that produce vectorization-friendly output:

Good styles (clean edges, binary):
- "linocut print"
- "woodcut illustration"
- "stencil art"
- "rubber stamp design"
- "silhouette"
- "pixel art" (for retro look)
- "paper cutout"
- "tribal tattoo style"
- "art deco geometric"
- "heraldic crest"
- "Japanese mon (family crest)"

Bad styles (gradients, detail, antialiasing):
- "photorealistic"
- "watercolor"
- "pencil sketch"
- "oil painting"
- "3D render"
- anything with "soft", "subtle", "gradient"

### Post-generation cleanup tips

If the AI didn't produce perfect black/white:

```bash
# Quick threshold with ImageMagick
convert input.png -colorspace Gray -threshold 50% -depth 1 clean.png

# Or with more control (adjust 45% to taste)
convert input.png -colorspace Gray -threshold 45% \
    -morphology Close Diamond:1 \
    -morphology Open Diamond:1 \
    clean.png
```

The morphology operations remove tiny speckles and fill small holes - exactly
what you want before vectorization.

---

## Stage 2: Vectorization - Pure Rust with vtracer

**Skip Inkscape entirely.** The `vtracer` crate is a pure Rust bitmap-to-SVG
vectorizer that's better than potrace for this use case:

- Pure Rust (no external dependency, compiles with your tool)
- O(n) algorithm vs potrace's O(n²)
- Handles both B/W and color input
- Produces clean polygon/spline output
- Configurable speckle filtering

### Integration in fabbula

```rust
// In Cargo.toml:
// vtracer = "0.6"

use vtracer::Config;

fn vectorize_image(input_png: &Path, output_svg: &Path) -> Result<()> {
    let config = Config {
        color_mode: vtracer::ColorMode::Binary,   // B/W mode
        hierarchical: vtracer::Hierarchical::Stacked,
        mode: vtracer::PathSimplifyMode::Polygon,  // No splines! Polygons only.
        filter_speckle: 4,       // Remove tiny artifacts
        corner_threshold: 60,    // Keep sharp corners sharp
        length_threshold: 4.0,
        splice_threshold: 45,
        path_precision: None,
        ..Default::default()
    };

    vtracer::convert_image_to_svg(
        input_png.to_str().unwrap(),
        output_svg.to_str().unwrap(),
        config,
    )?;
    Ok(())
}
```

**Critical setting: `mode: Polygon`** - this produces straight-line segments only.
Splines would need to be flattened later anyway, and polygon mode gives you
exact control over the geometry that ends up in GDSII.

### Alternative: potrace via CLI

If you prefer potrace (e.g., for its `--opttolerance` control):

```bash
# Convert to PBM first
convert input.png -colorspace Gray -threshold 50% input.pbm

# Vectorize with potrace
potrace input.pbm -s -o output.svg \
    --turdsize 5 \        # remove speckles < 5px
    --opttolerance 0.2 \  # curve optimization tolerance
    --longcurve \         # allow long Bézier segments
    --flat                # output polygons, not curves!
```

The `--flat` flag is key - it produces straight-line polygons directly.

---

## Stage 3: SVG Path → DRC-Clean Polygons

This is the most important stage. We need to:
1. Parse SVG paths into polylines
2. Scale paths to physical chip dimensions
3. Snap all coordinates to the manufacturing grid
4. Ensure all features meet DRC width/spacing rules
5. Optionally avoid existing top-metal structures

### Rust crate stack

```
usvg          - Parse SVG into simplified tree (resolves transforms, use, etc.)
svg2polylines - Flatten Bézier curves to polylines (uses lyon internally)
lyon          - Low-level path tessellation/flattening
geo           - Polygon area, boolean ops, simplification
geo-clipper   - Polygon union/difference (for exclusion zones)
gds21         - GDSII output
```

### The SVG→polygon pipeline

```rust
use usvg::{Tree, Options};

fn svg_to_polygons(svg_path: &Path, pdk: &PdkConfig, target_size_um: (f64, f64))
    -> Result<Vec<Vec<(i32, i32)>>>
{
    // 1. Parse SVG
    let svg_data = std::fs::read(svg_path)?;
    let tree = Tree::from_data(&svg_data, &Options::default())?;
    let svg_size = tree.size();  // SVG coordinate space

    // 2. Compute scale: SVG units → physical µm → database units
    let scale_x = target_size_um.0 / svg_size.width() as f64;
    let scale_y = target_size_um.1 / svg_size.height() as f64;
    let scale = scale_x.min(scale_y);  // uniform scaling
    let dbu_per_um = pdk.pdk.db_units_per_um as f64;
    let grid = pdk.grid.manufacturing_grid_um;

    // 3. Walk the SVG tree, extract filled paths
    let mut polygons = Vec::new();
    for node in tree.root().descendants() {
        if let usvg::NodeKind::Path(ref path) = *node.borrow() {
            if path.fill.is_some() {
                // Flatten Bézier curves to line segments
                let points: Vec<(i32, i32)> = flatten_path(&path.data)
                    .iter()
                    .map(|&(x, y)| {
                        // Scale to physical dimensions
                        let um_x = x as f64 * scale;
                        let um_y = y as f64 * scale;
                        // Snap to manufacturing grid
                        let snapped_x = (um_x / grid).round() * grid;
                        let snapped_y = (um_y / grid).round() * grid;
                        // Convert to database units
                        let dbu_x = (snapped_x * dbu_per_um).round() as i32;
                        let dbu_y = (snapped_y * dbu_per_um).round() as i32;
                        (dbu_x, dbu_y)
                    })
                    .collect();

                if points.len() >= 3 {
                    polygons.push(points);
                }
            }
        }
    }

    // 4. Post-process: remove tiny polygons, check widths
    let min_area_dbu2 = pdk.um_to_dbu(pdk.drc.min_width).pow(2) as f64;
    polygons.retain(|p| polygon_area(p).abs() > min_area_dbu2);

    Ok(polygons)
}

/// Flatten SVG path data (Bézier curves → line segments)
fn flatten_path(path_data: &usvg::PathData) -> Vec<(f64, f64)> {
    use lyon::path::PathEvent;
    use lyon::algorithms::walk::walk_along_path;
    // ... curve flattening with configurable tolerance
    // tolerance = min_width / 4 gives good results
    todo!()
}
```

### Two input modes for SVG

The tool should support two distinct SVG workflows:

**Mode A: AI-generated SVG (via vtracer)**
- Input is a vectorized bitmap → paths are already polygon outlines
- vtracer in `Polygon` mode means NO curves to flatten
- Just scale + snap + DRC filter

**Mode B: Hand-drawn / designer SVG**
- Input may contain Bézier curves, transforms, text, groups
- Need usvg to resolve everything
- Need lyon to flatten curves
- Need to handle strokes (convert to fill via stroke-to-path)

---

## Stage 4: Polygon → GDSII

Same as before - write each polygon as a `GdsBoundary` on the artwork layer.

For the SVG path (which can be arbitrary closed polygons, not just rectangles),
GDSII `GdsBoundary` supports arbitrary vertex lists:

```rust
fn polygon_to_gds_boundary(
    points: &[(i32, i32)],
    layer: i16,
    datatype: i16,
) -> GdsBoundary {
    let mut xy: Vec<GdsPoint> = points
        .iter()
        .map(|&(x, y)| GdsPoint::new(x, y))
        .collect();
    // GDSII requires closed polygons (first point repeated at end)
    if let Some(&first) = xy.first() {
        xy.push(first);
    }
    GdsBoundary {
        layer,
        datatype,
        xy,
        ..Default::default()
    }
}
```

**Important GDSII limits:**
- Max 8191 vertices per boundary (split larger polygons)
- Max 65535 elements per cell (use hierarchy for complex art)
- Coordinates are 32-bit signed integers

---

## Updated CLI Design

```
fabbula pipeline -i logo.png -o logo.gds -p sky130 \
    --size 200x200        # target artwork size in µm
    --threshold 50%       # binarization threshold (or "otsu")
    --vectorize polygon   # vtracer mode: polygon|spline|pixel
    --filter-speckle 4    # remove artifacts < 4px
    --svg-preview logo.svg # save intermediate SVG
    --check-drc           # validate output
```

Or step by step:

```
# Step 1: Vectorize
fabbula vectorize -i logo.png -o logo.svg --mode polygon --speckle 4

# Step 2: SVG to GDS
fabbula svg2gds -i logo.svg -o logo.gds -p sky130 --size 200x200

# Step 3: Merge into chip
fabbula merge --art logo.gds --chip my_chip.gds -o final.gds -p sky130 \
    --offset 50,30
```

---

## Updated Dependency Map

```toml
[dependencies]
# GDSII I/O
gds21 = "0.2"

# Vectorization (replaces potrace/inkscape!)
vtracer = "0.6"

# SVG parsing (for hand-drawn SVG input)
usvg = "0.44"

# Curve flattening
lyon_algorithms = "1"
lyon_path = "1"

# Image processing
image = "0.25"

# Geometry & polygon ops
geo = "0.28"
geo-clipper = "0.8"

# CLI
clap = { version = "4", features = ["derive"] }

# Config
serde = { version = "1", features = ["derive"] }
toml = "0.8"

# Error handling
anyhow = "1"
thiserror = "1"

# Logging
tracing = "0.1"
tracing-subscriber = "0.3"

# Parallelism
rayon = "1"
```

Everything is pure Rust. No Python, no Inkscape, no potrace binary, no
ImageMagick. One `cargo install fabbula` and you're done.

---

## Architecture Summary

```
fabbula/
├── Cargo.toml
├── pdks/
│   ├── sky130.toml
│   ├── ihp_sg13g2.toml
│   └── gf180mcu.toml
├── src/
│   ├── main.rs          # CLI (clap)
│   ├── lib.rs           # Public API
│   ├── pdk.rs           # PDK config loading + DRC rules
│   ├── artwork.rs       # Image loading + thresholding
│   ├── vectorize.rs     # NEW: vtracer integration (PNG→SVG)
│   ├── svg.rs           # NEW: SVG parsing + path flattening (SVG→polygons)
│   ├── polygon.rs       # Polygon generation + merging (bitmap path)
│   ├── snap.rs          # NEW: Grid snapping + DRC-aware sizing
│   ├── exclusion.rs     # NEW: Read existing metal, subtract from artwork
│   ├── gdsio.rs         # GDSII read/write via gds21
│   ├── drc.rs           # DRC validation
│   └── preview.rs       # SVG preview output
└── examples/
    ├── prompt_templates/ # AI prompt templates for different styles
    └── sample_art/       # Example input images + expected output
```

---

## What makes this better than everything else

| Feature | fabbula | ArtistIC | logo-to-gds2 | png2gds |
|---------|---------|----------|--------------|---------|
| Single binary, zero deps | ✅ Rust | ❌ Python+KLayout+IM | ❌ Python+Magic | ❌ C+libpng |
| Built-in vectorizer | ✅ vtracer | ❌ external | ❌ external | ❌ none |
| SVG input | ✅ usvg | ❌ bitmap only | ✅ via Magic | ❌ |
| AI prompt guide | ✅ | ❌ | ❌ | ❌ |
| Multi-PDK | ✅ 3+ built-in | ❌ IHP only | ❌ SKY130 only | ❌ |
| DRC-clean output | ✅ grid snap | ✅ tetromino | ❌ | ❌ |
| Built-in DRC check | ✅ | ❌ | ❌ | ❌ |
| Exclusion zones | 🔜 planned | ✅ | ❌ | ❌ |
| Vector polygon output | ✅ arbitrary | ❌ rects only | ❌ rects | ❌ rects |
| End-to-end pipeline | ✅ PNG→GDS | ❌ manual steps | ❌ manual | ❌ manual |
