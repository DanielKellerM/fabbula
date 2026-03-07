# fabbula - TODO Tracker

> Tracking file for Claude Code. Each task has a status checkbox, priority, and context to enable autonomous work.

## Legend

- `[ ]` Open - not started
- `[~]` In progress
- `[x]` Done
- Priority: **P0** (blocker) / **P1** (high) / **P2** (nice-to-have)

---

## In Progress

- [x] **P1** Tile-based deep zoom previews
  - Google Maps-style deep zoom: PNG tile pyramid at overview, SVG polygons when zoomed in
  - `src/tiles.rs`: tile pyramid generation via tiny_skia, density grid, polygon JSON
  - `src/preview.rs`: deep zoom HTML viewer with canvas tile renderer + SVG overlay
  - `src/main.rs`: `--deep-zoom` CLI flag for Generate and Merge commands
  - Files: `src/tiles.rs`, `src/preview.rs`, `src/main.rs`, `src/lib.rs`, `docs/index.html`

- [x] **P0** Audit fixes - competitive analysis and usability review
  - A1: Fix wide_metal_spacing in pitch calculation (DRC-by-construction bug)
  - A2: Fix README to describe actual pipeline (not fake vectorization)
  - B1: Add `--size-um` CLI parameter for physical dimension specification
  - B2: Add artwork bounds reporting after generation/merge
  - B5: Better error messages with available cell list
  - B4: Compressed GDS support (.gds.gz via flate2)
  - Files: `src/polygon.rs`, `src/pdk.rs`, `README.md`, `src/main.rs`, `src/gdsio.rs`, `Cargo.toml`

- [x] **P0** HFT-style performance optimization (Phase 1)
  - Phase 0: Added density-only bench + profiling binary for flame graphs
  - Phase 1: GreedyMerge u16 runs, word-level bitops, direct bitset access
  - Phase 2: Parallel runs computation + strip-based parallel greedy merge
  - Phase 3: SAT-based density checking (replace R-tree queries)
  - Phase 4: DRC zero-alloc iterators, u32 IndexedRect, #[inline] on hot paths
  - Files: `src/polygon.rs`, `src/artwork.rs`, `src/drc.rs`, `profiling/bench_*.rs`

- [x] **P1** Performance optimization Phase 2
  - Phase 1: Benchmark baseline (bench_artwork.rs)
  - Phase 2: Word-level SAT construction + count_on_in_window with popcount
  - Phase 3: Incremental SAT in enforce_density (reuse buffer, rebuild from dirty row)
  - Phase 4: DRC grid rasterization interior/border split
  - Phase 5: Easy wins (gdsio reserve, drc alloc-free par_iter, polygon thread-local bitset)
  - Files: `src/artwork.rs`, `src/drc.rs`, `src/gdsio.rs`, `src/polygon.rs`, `profiling/bench_artwork.rs`

- [x] **P1** Add fabbula2 PDK + GPU die-scale demo
  - Imaginary 2nm nanosheet PDK inspired by TSMC N2
  - AP layer: min_width 0.80 um, min_spacing 0.80 um
  - GPU die-scale demo: B200-class ~814 mm2 (28.5mm x 28.5mm)
  - Files: `pdks/fabbula2.toml`, `src/pdk.rs`

- [x] **P0** Enforce max_width during polygon merge
  - Compute max_merge pixel cap from max_width, pixel_w, pitch
  - Cap row_merged_rects run length and greedy_merge width/height
  - Eliminates max_width DRC violations for fabbula2, ASAP7
  - Files: `src/polygon.rs`

- [x] **P1** Performance optimization Phase 3: Exclusion mask + neighbor counting
  - Scan-line rasterization for `apply_exclusion_mask` (replaces R-tree per-pixel queries)
  - `bulk_clear_bits` and `count_bits_in_range` word-level helpers
  - Inlined `count_neighbors` with direct word access (no bounds checks)
  - Results: enforce_density -25% to -26%, exclusion mask ~500ns for 512x512
  - Files: `src/artwork.rs`, `profiling/bench_artwork.rs`

- [x] **P1** Competitive audit fixes (round 2)
  - PDK TOML fixes: added source citations to IHP/GF180, GF180 max_width=30.0 slotting rule
  - README: tapeout integration section (safety, merge workflow, PDK confidence)
  - `--exclusion-layer LAYER/DATATYPE` flag for merge command (custom exclusion layer)
  - `--dither` flag: Floyd-Steinberg dithering for gradient artwork
  - SVG input support via resvg (auto-detected by .svg extension)
  - Files: pdks/*.toml, README.md, src/main.rs, src/artwork.rs, src/color.rs, src/gdsio.rs, Cargo.toml

## Features

- [ ] **P2** Smart auto-polarity and threshold improvements
  - Current: Otsu threshold with "dark = metal" convention, manual `--invert` flag
  - Auto-polarity: if >50% pixels are metal after thresholding, auto-invert (subject is likely bright on dark bg)
  - Explore adaptive thresholding (e.g. Sauvola, Niblack) for images with uneven lighting
  - Consider edge detection (Canny/Sobel) as alternative to luminance thresholding for line art
  - Metal-layer-aware: real metal appears bright/reflective in microscopy - could default to "bright = metal"
  - Color-aware thresholding: use saturation/hue channels, not just luminance, for colored artwork
  - Histogram analysis: detect bimodal vs unimodal distributions and warn when Otsu is unreliable
  - Files: `src/artwork.rs`

- [x] **P1** Add color/multi-layer support from PDK layer maps
  - `ArtworkLayerProfile` struct with per-layer DRC in `src/pdk.rs`
  - `[[artwork_layers]]` TOML format (backward-compatible with existing PDKs)
  - `src/color.rs`: channel (R/G/B) and palette (k-means) extraction modes
  - `--color-mode single|channel|palette` and `--num-colors N` CLI flags
  - Multi-layer GDS, SVG/HTML preview with layer colors/legend, LEF output
  - Per-layer DRC checking and density enforcement

- [x] **P1** GDS import for merge workflow
  - Hierarchy flattening: recursive SREF/AREF traversal with Transform composition
  - Arbitrary polygon support: bounding box from all vertices (not just 5-point rects)
  - GDS path elements: bounding box with width/2 expansion
  - Multi-layer exclusion: channel/palette merge modes now apply exclusion mask
  - Depth-limited recursion (max 64) to handle cyclic refs safely
  - 9 unit tests: SREF offset, arbitrary polygon bbox, path width, AREF 2x2, depth limit, 90/180/270 rotation, reflection
  - Files: `src/gdsio.rs` (Transform, flatten_cell, read_existing_metal), `src/main.rs` (merge exclusion)

- [x] **P2** Add virtual PDK support (FreePDK45 + ASAP7)
  - Two most useful open-source virtual PDKs for academic/research flows
  - **FreePDK45** (45nm, NCSU):
    - Top metal: Metal10, GDS layer 21/0
    - Min width: 0.800 um, min spacing: 0.800 um
    - Rules from NCSU/mflowgen repos
    - File: `pdks/freepdk45.toml`
  - **ASAP7** (7nm FinFET, ASU/ARM):
    - Top metal: M9 (single-patterned)
    - Min width: ~0.040 um (40nm), spacing varies by context (~40-60nm)
    - Layer numbers need extraction from ASAP7 PDK repo (asap7_layermap)
    - File: `pdks/asap7.toml`
  - Also: update `src/pdk.rs` to `include_str!` new TOMLs, add to `list-pdks` output,
    add integration test cases in `tests/drc_per_pdk.rs`, update README Supported PDKs table

- [x] **P2** Support compressed GDS input (.gds.gz)
  - Transparent decompression in `load_gds()` via `flate2` crate
  - Detect by `.gz` extension, decompress to temp file before loading
  - Files: `src/gdsio.rs`, `Cargo.toml`

## Performance / Profiling

- [x] **P2** Die-scale benchmarks for profiling suite
  - Separate benchmark group (`cargo bench --bench bench_die_scale`)
  - 10mm die (~6250px) and 25mm die (~15625px) at sky130 pitch
  - Benchmarks: greedy_merge_6250, greedy_merge_15625, drc_6250
  - Files: `profiling/bench_die_scale.rs`, `Cargo.toml`

- [x] **P1** Add LICENSE files, copyright headers, and IIS ETHZ attribution
  - Apache 2.0 LICENSE for software, Solderpad Hardware License v2.1 for PDK configs
  - Copyright headers on all src/*.rs and pdks/*.toml files
  - README: fabbula2 in PDK table, IIS ETHZ/CSEM acknowledgment, dual license section
  - Files: LICENSE, LICENSE.SHL, README.md, src/*.rs, pdks/*.toml

## Infrastructure

- [x] **P2** Set up GitHub Pages deployment
  - Enable Pages in repo settings: Source = "Deploy from branch", Branch = `main`, Folder = `/docs`
  - `docs/index.html` and `docs/previews/` already committed

## Open-Source Readiness

### P0 - Correctness

- [ ] **P0** `min_enclosed_area` loaded but never checked
  - Field is deserialized from TOML (`pdk.rs:50`) and appears in test structs, but no DRC check in `drc.rs` uses it
  - Dead code that implies a check exists when it doesn't
  - Either implement the check or remove the field and document it as unsupported
  - Files: `src/drc.rs`, `src/pdk.rs`

- [ ] **P0** Path bbox expansion over-expands in path direction
  - `gdsio.rs:365-376`: `half_w` is applied to all vertices' x AND y coordinates uniformly
  - A horizontal path (0,0)-(100,0) with width 40 should produce bbox (0,-20,100,20) but produces (-20,-20,120,20)
  - Creates larger exclusion zones than necessary during merge
  - Files: `src/gdsio.rs`

- [ ] **P0** LEF output is bounding-box only, not real geometry
  - `lef.rs:59`: writes a single RECT covering the full artwork bounding box per layer
  - Router sees one giant obstacle instead of the actual artwork shape
  - OpenLane integration claim in README is misleading - LEF won't allow routing through gaps
  - Either fix to emit actual polygon geometry or clarify the limitation in README
  - Files: `src/lef.rs`, `README.md`

### P1 - Robustness

- [ ] **P1** `most_conservative_drc()` missing max_width/wide_metal propagation
  - In multi-layer mode, `shared_drc` (`main.rs:273-281`) copies `profiles[0].drc` and only updates `min_width`, `min_spacing`, `min_area`
  - `max_width`, `wide_metal_threshold`, `wide_metal_spacing` from alt layers are silently lost
  - If profiles[0] has no max_width but an alt layer does, no max_width capping occurs during polygon generation
  - Per-layer DRC check catches violations post-generation, but "clean by construction" guarantee is broken
  - Fix: take min of max_width across profiles, max of wide_metal values
  - Files: `src/main.rs`

- [ ] **P1** Palette mode num_colors > profiles.len() silent fallback
  - `main.rs:661`: `profiles.get(layer_index).unwrap_or(&profiles[0])` means excess colors get written to profiles[0]'s GDS layer
  - Multiple k-means clusters silently merge onto the same layer
  - Fix: error or warn when `num_colors > profiles.len()`
  - Files: `src/main.rs`

- [ ] **P1** AREF instance coordinate overflow during GDS import
  - `gdsio.rs:428-429`: `c * col_pitch_x + r * row_pitch_x` uses no checked arithmetic
  - Large AREF (e.g. 10000 cols x 100um pitch = 1B dbu) overflows i32, producing corrupted exclusion zones
  - Distinct from general coordinate overflow (line 188) - this is specifically about GDS import flattening
  - Fix: use `checked_mul`/`checked_add` or validate before loop
  - Files: `src/gdsio.rs`

- [ ] **P1** README "DRC-clean by construction" overclaim in comparison table
  - `README.md:207`: comparison table says "DRC-clean output: By construction" without qualification
  - Only min_width and min_spacing are guaranteed by construction (pixel grid)
  - max_width relies on post-generation capping, density on pre-pass enforcement, min_area on post-filtering
  - The disclaimer section (line 283) is good but the marketing table is misleading
  - Fix: change to "By construction (width/spacing)" or add footnote
  - Files: `README.md`

- [ ] **P1** No image size guard
  - A 100k x 100k all-black image produces 10 billion pixels and potentially millions of polygons
  - No maximum image dimension or polygon count limit; could OOM or hang
  - Add a configurable max dimension (e.g. 10000px default) with `--max-pixels` override
  - Files: `src/artwork.rs`, `src/main.rs`

- [ ] **P1** No PDK cross-field validation
  - `density_min` not validated to be < `density_max`
  - `min_area` and `min_enclosed_area` not validated as non-negative
  - No check that `db_units_per_um > 0`
  - Add validation in `PdkConfig::load()` or a dedicated `validate()` method
  - Files: `src/pdk.rs`

- [ ] **P1** Temp file handling in gdsio.rs uses PID-based naming
  - `gdsio.rs:29-44`: uses `format!("fabbula_decompress_{}.gds", std::process::id())` instead of `tempfile::NamedTempFile`
  - PID-based naming can collide; file persists on early error
  - Replace with `tempfile` crate for safe temp file handling
  - Files: `src/gdsio.rs`, `Cargo.toml`

- [ ] **P1** GDS coordinate overflow for large designs
  - i32 coordinates at 1000 DBU/um overflow at ~2.1mm
  - A 10mm chip at 1nm DBU (1M DBU/um) overflows i32 with no warning
  - Add overflow check when computing coordinates, warn or error if exceeded
  - Files: `src/polygon.rs`, `src/pdk.rs`

- [ ] **P1** No end-to-end CLI tests
  - No test runs `fabbula generate` or `fabbula merge` with a real image and validates output GDS
  - Add integration tests that exercise full CLI pipeline
  - Files: `tests/`

### P2 - Improvements

- [ ] **P2** No rotation/flip support
  - Artwork must be pre-oriented in the input image
  - Add `--rotate` (0/90/180/270) and `--flip` (horizontal/vertical) CLI flags
  - Files: `src/artwork.rs`, `src/main.rs`

- [ ] **P2** Single-layer artwork per invocation
  - Can't natively produce multi-metal-height artwork (e.g. met4 + met5 aligned)
  - Color modes put different colors on different layers, but can't specify arbitrary layer targets
  - Files: `src/main.rs`, `src/pdk.rs`

- [ ] **P2** No CI matrix for cross-platform or MSRV testing
  - Only tests on ubuntu-latest with one Rust version
  - Add macOS and Windows runners, test against MSRV (1.93)
  - Files: `.github/workflows/ci.yml`

- [ ] **P2** No `cargo audit` in CI
  - No security/dependency vulnerability scanning
  - Add `cargo audit` step to CI pipeline
  - Files: `.github/workflows/ci.yml` or new workflow

- [ ] **P2** max_width enforced by splitting, not slotting
  - GF180MCU has `max_width=30.0` per DRM 14.6.3 which requires slotting (holes in wide metal)
  - Current impl splits wide rects into smaller ones during merge, but doesn't create real slots
  - Document limitation or implement basic slotting
  - Files: `src/polygon.rs`, docs

- [ ] **P2** Density enforcement can fail silently
  - If the density loop doesn't converge after 3 retries, it warns but continues
  - User might tapeout with density violations without realizing
  - Consider making non-convergence an error (with `--force` override)
  - Files: `src/artwork.rs`

- [ ] **P2** Document which DRC rules are and aren't checked
  - No enclosure, antenna, via, acute angle, off-grid, or same-net spacing checks
  - Expected for an artwork tool, but README should explicitly list supported vs unsupported checks
  - Files: `README.md`

- [ ] **P2** Clarify LEF output limitations in README
  - Roadmap shows LEF output as complete, but it's bounding-box only
  - Either improve or clearly document the limitation
  - Files: `README.md`

- [ ] **P2** min_area sqrt rounding - use direct dbu^2 calculation
  - `polygon.rs:156`: `um_to_dbu(sqrt(min_area))^2` introduces unnecessary rounding vs direct conversion
  - ~1% error for sub-um geometries (ASAP7: 7921 vs 8000 dbu^2), negligible for mature nodes
  - Fix: `(drc.min_area * (dbu_per_um as f64).powi(2)) as i64`
  - Files: `src/polygon.rs`

- [ ] **P2** GF180MCU min_spacing DRM verification needed
  - `pdks/gf180mcu.toml:29`: TOML says 0.46um, one source claims DRM 14.6.2 specifies 0.44um
  - 0.02um difference matters at 180nm - needs verification against official DRM document
  - Files: `pdks/gf180mcu.toml`

- [ ] **P2** Density window silently skipped when pitch > window
  - `main.rs:490-496`: when `density_window_um / pitch_um < 1`, `window_px = 0` and enforcement is skipped with no log
  - User might expect density to be enforced but it's silently disabled
  - Fix: log warning when window_px == 0
  - Files: `src/main.rs`

- [ ] **P2** K-means convergence feedback
  - `color.rs:223-304`: fixed 15-iteration limit with no log of whether convergence was reached
  - Could produce poor layer separation without any user visibility
  - Fix: log iteration count; warn if max iterations reached
  - Files: `src/color.rs`

- [ ] **P2** PDK layer number collision validation
  - If artwork_layer and artwork_layer_alt have the same gds_layer number, silently creates duplicate profiles
  - User error but easy to catch during PDK load
  - Fix: validate uniqueness in `PdkConfig::load()`
  - Files: `src/pdk.rs`

- [ ] **P2** README "vectorize" language leftover
  - `README.md:137`: "Styles that vectorize well" - fabbula doesn't vectorize, it rasterizes to bitmap then generates rects
  - Leftover from before previous audit fixed the pipeline description
  - Fix: change to "Styles that work well" or "Styles that convert well"
  - Files: `README.md`

## Completed

- [x] Make touching mode the default, add `--separated` opt-out flag
- [x] Logo + example gallery + GitHub Pages preview gallery (`58d37bf`)
- [x] README header redesign with logo and poem
- [x] Generate SVG/HTML outputs for all input images across PDKs
- [x] Fix `count_on_in_window` build error in `src/artwork.rs`
