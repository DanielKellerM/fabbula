# Future: SVG Vectorization Pipeline

> **Status: Design document / future work.** This pipeline is not yet implemented.
> fabbula currently uses raster-to-rectangle conversion (bitmap -> merged rectangles).
> The SVG vectorization path described below is a planned enhancement.

## Stage 1: AI Image Generation - Prompt Engineering

The key insight: chip artwork lives on a single metal layer, which means it's
fundamentally monochrome - the metal is either there or it isn't. Your AI prompt
needs to produce images that vectorize cleanly into solid black/white regions.

### What makes a good chip art source image

- **High contrast** - pure black on white (or vice versa)
- **No gradients** - hard edges, solid fills
- **No anti-aliasing artifacts** - the cleaner the edge, the fewer polygons
- **Bold shapes** - fine detail below ~2um will be lost (that's the minimum feature size)
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

## Stage 2: Vectorization (Not Yet Implemented)

The plan is to use the `vtracer` crate (pure Rust bitmap-to-SVG vectorizer)
to convert binarized images into SVG polygon paths before snapping to the
DRC grid. This would produce smoother, more faithful reproductions of curved
artwork compared to the current pixel-rectangle approach.

Key parameters for vtracer integration:
- `ColorMode::Binary` for B/W mode
- `PathSimplifyMode::Polygon` to produce straight-line segments only
- `filter_speckle` to remove tiny artifacts
- `corner_threshold` to keep sharp corners

---

## Stage 3: SVG Path to DRC-Clean Polygons (Not Yet Implemented)

This stage would parse SVG paths into polylines, scale to physical chip
dimensions, snap coordinates to the manufacturing grid, and ensure all
features meet DRC width/spacing rules.

Planned crate stack:
- `usvg` - parse SVG into simplified tree
- `lyon` - flatten Bezier curves to polylines
- `geo` / `geo-clipper` - polygon boolean operations

Two input modes are envisioned:
- **Mode A: AI-generated SVG (via vtracer)** - paths are already polygon outlines,
  just need scale + snap + DRC filter
- **Mode B: Hand-drawn / designer SVG** - may contain Bezier curves, transforms,
  text, and groups requiring full SVG resolution and curve flattening

---

## Stage 4: Polygon to GDSII

Same as the current pipeline - write each polygon as a `GdsBoundary` on the
artwork layer. The SVG path would produce arbitrary closed polygons rather than
only axis-aligned rectangles.

GDSII limits to keep in mind:
- Max 8191 vertices per boundary (split larger polygons)
- Max 65535 elements per cell (use hierarchy for complex art)
- Coordinates are 32-bit signed integers
