# WASM Browser App - Implementation Plan

> Scope boundary: fabbula is a manufacturing-aware placer/converter.
> Artwork authoring stays in external tools (Inkscape/Figma/Illustrator), then assets are
> imported into fabbula for die-aware placement, DRC-aware generation, and export.

## Competitive Position

| Capability | Existing tools | fabbula WASM |
|------------|---------------|--------------|
| Image to GDS in browser | None | First |
| Client-side GDSII generation | None | First |
| Die-aware placement + DRC-safe conversion | Fragmented scripts/tools | Unified |
| DRC-aware anything in browser | None | First |
| QR to DRC-clean GDS | LayoutEditor (commercial desktop, no DRC) | First DRC-aware, first in browser |
| Text/font to DRC-clean GDS | gdstk, PHIDL (Python libs, no DRC) | First DRC-aware, first in browser |
| Custom PDK in browser | None | First |

## Architecture

```
┌─────────────────────────────────────────────────┐
│  Placement Canvas (JS / Canvas2D)               │
│  - Import authored SVG/PNG assets               │
│  - Import chip GDS and derive die/canvas bounds │
│  - Move/scale placed image with handles         │
│  - Optional simple text/QR labels               │
├─────────────────────────────────────────────────┤
│  WASM Pipeline (Rust -> wasm32-unknown-unknown)  │
│  - Reads placement bitmap from canvas ImageData  │
│  - threshold -> bitmap -> polygons -> DRC check  │
│  - Returns rect array + DRC results + stats      │
├─────────────────────────────────────────────────┤
│  Preview Canvas (WebGL or Canvas2D)              │
│  - Renders polygons as rectangles                │
│  - Pan/zoom viewer (reuse existing HTML viewer)  │
│  - Layer toggle, DRC violation highlights        │
│  - Stats bar: polygon count, dimensions, density │
└─────────────────────────────────────────────────┘
```

All processing is client-side. GDS files never leave the user's machine.
Static HTML hosted on GitHub Pages - zero backend, zero security concerns.

## Performance Budget

Native benchmarks (M4 Mac):

| Step | 256x256 | 512x512 | 1024x1024 |
|------|---------|---------|-----------|
| Threshold + bitmap | ~0.1ms | ~0.3ms | ~1ms |
| Greedy merge | 0.18ms | 0.72ms | ~3ms |
| DRC check | ~0.5ms | ~2ms | ~7ms |
| **Total** | **~0.8ms** | **~3ms** | **~11ms** |

WASM overhead: 1.5-3x slower (no SIMD initially, single-threaded).

| | 256x256 | 512x512 | 1024x1024 |
|--|---------|---------|-----------|
| WASM estimate | ~2ms | ~8ms | ~30ms |
| FPS equivalent | 500 | 125 | 33 |

Strategy: preview at 256-512px during interaction (instant), full resolution on release.

## Interaction Strategy

- During move/scale drag: pipeline at preview resolution (~256-768px), update live
- On mouse release: full-resolution refinement run
- Placement/PDK/strategy changes: debounced preview, queued full-res latest-only
- "Download GDS" button: full resolution, write GDS bytes, trigger browser download

---

## Implementation Phases

### Phase 0: Rust Preparation

> Make the library WASM-compatible without breaking CLI.

- [ ] Add `PdkConfig::from_toml_str(content: &str)` public method
  - `from_file()` already does `read_to_string` then `toml::from_str` - just expose the inner part
  - Files: `src/pdk.rs`

- [ ] Add feature gates to `Cargo.toml`
  ```toml
  [features]
  default = ["cli"]
  cli = ["clap", "tracing-subscriber"]
  wasm = ["wasm-bindgen", "js-sys"]
  ```
  - `rayon` must be optional (no threads in WASM) - feature-gate parallel DRC
  - `image` crate works in WASM (no file I/O needed, decode from bytes)
  - `gds21` write must return `Vec<u8>` instead of writing to disk
  - Files: `Cargo.toml`, `src/lib.rs`, `src/drc.rs` (rayon gate), `src/gdsio.rs` (byte output)

- [ ] Add `write_gds_to_bytes()` function
  - Same as `write_gds_multi` but returns `Vec<u8>` instead of writing to Path
  - gds21 serialization to in-memory buffer
  - Files: `src/gdsio.rs`

- [ ] Add `threshold_from_pixels(pixels: &[u8], width: u32, height: u32)` function
  - Skip image crate decode - canvas already gives RGBA pixels
  - Convert RGBA ImageData directly to ArtworkBitmap
  - Files: `src/artwork.rs`

- [ ] Unit tests: verify pipeline works without rayon, verify byte output matches file output

### Phase 1: WASM Entry Point

> Minimal working WASM build that takes pixels in and returns rects out.

- [ ] Create `src/wasm.rs` with `#[wasm_bindgen]` entry points:
  ```rust
  #[wasm_bindgen]
  pub fn generate_from_pixels(
      pixels: &[u8],        // RGBA from canvas ImageData
      width: u32,
      height: u32,
      pdk_name: &str,       // built-in name or "custom"
      custom_pdk_toml: &str, // empty string if using built-in
      strategy: &str,
      invert: bool,
      dither: bool,
  ) -> JsValue  // { rects: Float32Array, violations: [], stats: {} }

  #[wasm_bindgen]
  pub fn generate_gds_bytes(
      pixels: &[u8], width: u32, height: u32,
      pdk_name: &str, custom_pdk_toml: &str,
      strategy: &str, invert: bool, dither: bool,
      cell_name: &str,
  ) -> Vec<u8>  // GDS file bytes for download

  #[wasm_bindgen]
  pub fn validate_pdk_toml(toml_content: &str) -> JsValue
  // { valid: bool, error: string|null, info: { name, pitch, layers, ... } }

  #[wasm_bindgen]
  pub fn list_builtin_pdks() -> JsValue
  // [{ name, description, node_nm, min_width, min_spacing, ... }]
  ```

- [ ] Build with `wasm-pack build --target web --features wasm`
- [ ] Verify WASM binary size (target: <2MB after wasm-opt)
- [ ] Smoke test: load in browser, pass test pixels, verify rect output

### Phase 2: Minimal Browser UI

> Import asset, pick PDK, place against die bounds, preview polygons, download GDS.

- [ ] Create `wasm/index.html` - single self-contained HTML file
  - PDK selector (tabs for 6 built-in + "Custom" tab)
  - Asset drop zone (drag-drop or file picker)
  - Optional chip GDS input to set die/canvas bounds
  - Strategy dropdown (greedy-merge default)
  - Invert / dither toggles
  - "Generate" button (or auto-generate on drop)

- [ ] Canvas-based polygon preview
  - Render returned rects as filled rectangles on Canvas2D
  - Pan (drag) and zoom (scroll wheel)
  - Dark background, metal color from PDK
  - Reuse visual style from existing HTML viewer

- [ ] Stats bar
  - Polygon count, artwork dimensions (um and mm), density %, DRC status
  - Update on every generation

- [ ] GDS download button
  - Calls `generate_gds_bytes()`, creates Blob, triggers `<a download>` click
  - File named `artwork_{pdk}.gds`

- [ ] DRC results panel
  - Green badge: "DRC clean" with checkmark
  - Red badge: violation count + expandable list

### Phase 3: Real-Time Placement

> Core feature: place/scale imported artwork over real die geometry with live preview.

- [ ] Placement canvas (left panel)
  - Imported image layer: drag to position, corner handles to resize
  - Aspect ratio lock toggle
  - Canvas bounds driven by input GDS die extents when provided
  - Canvas composites placement bitmap for WASM input

- [ ] Real-time pipeline loop
  ```js
  function onCompositionChange() {
      const pixels = compositeCanvas.getImageData(0, 0, w, h);
      // Use requestAnimationFrame + debounce
      const result = wasm.generate_from_pixels(pixels.data, w, h, pdk, ...);
      previewCanvas.renderRects(result.rects);
      updateStats(result.stats);
      updateDrc(result.violations);
  }
  ```
  - During drag: generate preview resolution (debounce for responsiveness)
  - On release: generate at full resolution

- [ ] Optional simple label overlays (text/QR)
  - Single-purpose metadata/traceability labels
  - Keep controls minimal (payload, size, placement), no rich editor features

### Phase 4: Custom PDK Editor

> Upload or edit PDK TOML in the browser with live validation.

- [ ] "Custom PDK" tab opens inline TOML editor
  - Textarea with monospace font, syntax highlighting (optional)
  - Pre-filled with template:
    ```toml
    [pdk]
    name = "my_process"
    node_nm = 65
    db_units_per_um = 1000
    description = "My custom process"

    [artwork_layer]
    name = "top_metal"
    gds_layer = 100
    gds_datatype = 0

    [drc]
    min_width = 0.4
    min_spacing = 0.4

    [grid]
    manufacturing_grid_um = 0.001
    ```

- [ ] Live validation on every keystroke
  - Calls `wasm.validate_pdk_toml(text)` (microseconds)
  - Green: "Valid - pixel pitch: 0.800 um"
  - Red: inline error ("min_width must be > 0")

- [ ] "Use template" dropdown: copy from any built-in PDK as starting point
- [ ] "Upload .toml" button: reads file, populates editor
- [ ] Custom PDK persists in localStorage between sessions

### Phase 5: Polish and Ship

> Production-ready app on GitHub Pages.

- [ ] Responsive layout (works on tablet, degrades gracefully on mobile)
- [ ] Loading state: show progress while WASM module loads (~1-2s first visit)
- [ ] Error handling: friendly messages for invalid images, oversized files, WASM failures
- [ ] Keyboard shortcuts: Ctrl+Z undo composition, Ctrl+S download GDS
- [ ] URL params: `?pdk=sky130&demo=true` loads with sample image
- [ ] Sample images: include 3-4 built-in demo images (base64 or fetched)
- [ ] PWA manifest: installable as desktop app (optional)
- [ ] GitHub Pages deployment: add to existing `docs/` or separate `wasm/dist/`
- [ ] Link from main gallery page and README

---

## File Structure

```
wasm/
  index.html          # Single-page app (HTML + inline CSS + JS)
  build.sh            # wasm-pack build + wasm-opt + copy to docs/
src/
  wasm.rs             # #[wasm_bindgen] entry points
  lib.rs              # Feature-gated module inclusion
  pdk.rs              # from_toml_str() addition
  gdsio.rs            # write_gds_to_bytes() addition
  artwork.rs           # threshold_from_pixels() addition
  drc.rs              # Feature-gate rayon
docs/
  app/                # Built WASM app served by GitHub Pages
    index.html
    fabbula_bg.wasm
    fabbula.js
```

## Dependencies (WASM-specific)

```toml
[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen = "0.2"
js-sys = "0.3"
serde-wasm-bindgen = "0.6"  # Rust structs -> JsValue
```

No new runtime dependencies for the CLI. WASM deps only compile for wasm32 target.

## Open Questions

- [ ] WASM binary size: gds21 + image + toml + serde could be large. Profile with `twiggy`.
  Target <2MB after wasm-opt. If too large, consider lazy-loading gds21 (only for download).
- [ ] Keep text rendering scope minimal: Rust raster font options vs JS text; avoid rich typography/editor scope.
- [ ] QR generation path: JS-side library or Rust `qrcode` in WASM; whichever keeps binary lean and deterministic.
- [ ] Multi-layer color modes: support channel/palette in WASM or single-layer only for v1?
  Start with single-layer, add multi-layer in a follow-up.
- [ ] SVG preview vs Canvas2D for polygon rendering: Canvas2D is simpler and faster for
  >10k rects. SVG better for <1k rects with zoom. Start with Canvas2D.
