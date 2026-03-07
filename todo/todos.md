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

- [x] **P0** `min_enclosed_area` loaded but never checked
  - Removed: fabbula generates solid rectangles only, enclosed areas can't exist by construction
  - Removed field from DrcRules struct, all 6 PDK TOMLs, and test helpers
  - Custom TOMLs with the field still parse fine (serde ignores unknown fields)

- [x] **P0** Path bbox expansion over-expands in path direction
  - Fixed: 2-point axis-aligned paths now expand only perpendicular to path direction
  - Multi-segment/diagonal paths still use conservative all-direction expansion
  - Added test_path_vertical test; updated test_path_element_with_width expectations

- [x] **P0** LEF output is bounding-box only, not real geometry
  - Fixed: LEF now emits actual per-rect geometry instead of single bounding box
  - Router can now route through gaps in artwork
  - Coordinates offset relative to macro origin (bbox corner)

### P1 - Robustness

- [x] **P1** `most_conservative_drc()` missing max_width/wide_metal propagation
  - Fixed: now propagates max_width (min), wide_metal_threshold (min), wide_metal_spacing (max) across profiles

- [x] **P1** Palette mode num_colors > profiles.len() silent fallback
  - Fixed: anyhow::ensure! errors when num_colors exceeds available profiles
  - Applied to both generate and merge palette paths; replaced .unwrap_or fallback with direct index

- [x] **P1** AREF instance coordinate overflow during GDS import
  - Fixed: replaced arithmetic with saturating_mul/saturating_add to prevent i32 overflow
  - Overflowing coordinates clamp to i32::MAX/MIN instead of wrapping

- [x] **P1** README "DRC-clean by construction" overclaim in comparison table
  - Fixed: changed to "By construction (width/spacing)" to clarify scope

- [x] **P1** No image size guard
  - Fixed: added 16384x16384 max dimension check in load_artwork()
  - Error message directs user to --max-width/--max-height for resize

- [x] **P1** No PDK cross-field validation
  - Fixed: added db_units_per_um > 0, min_area >= 0, density_min in [0,1], density_min <= density_max

- [x] **P1** Temp file handling in gdsio.rs uses PID-based naming
  - Fixed: replaced with tempfile::NamedTempFile; auto-cleaned on drop

- [x] **P1** GDS coordinate overflow for large designs
  - Fixed: added i32 overflow check in generate_polygons before rect computation
  - Error message suggests reducing image size or using coarser PDK grid

- [x] **P1** Touching mode violates DRC for GF180MCU and FreePDK45
  - Fixed: touching mode pitch changed from min_width to max(min_width, effective_spacing)
  - Guarantees spacing even when min_width < min_spacing or wide_metal_spacing

- [x] **P1** No end-to-end CLI tests
  - Added tests/end_to_end.rs with 5 tests: real image pipeline, all-PDKs separated mode,
    empty bitmap, single pixel, all strategies DRC-clean

### P2 - Improvements

- [ ] **P2** No rotation/flip support
  - Artwork must be pre-oriented in the input image
  - Add `--rotate` (0/90/180/270) and `--flip` (horizontal/vertical) CLI flags
  - Files: `src/artwork.rs`, `src/main.rs`

- [ ] **P2** Single-layer artwork per invocation
  - Can't natively produce multi-metal-height artwork (e.g. met4 + met5 aligned)
  - Color modes put different colors on different layers, but can't specify arbitrary layer targets
  - Files: `src/main.rs`, `src/pdk.rs`

- [x] **P2** No CI matrix for cross-platform or MSRV testing
  - Added macOS runner to build matrix (ubuntu-latest + macos-latest)
  - Also added coverage job (cargo-llvm-cov, 70% threshold) and cargo audit job
  - Files: `.github/workflows/ci.yml`

- [x] **P2** No `cargo audit` in CI
  - Added cargo-audit job to CI pipeline via taiki-e/install-action
  - Files: `.github/workflows/ci.yml`

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

- [x] **P2** Clarify LEF output limitations in README
  - Fixed: LEF now emits actual geometry, no longer bounding-box only

- [x] **P2** min_area sqrt rounding - use direct dbu^2 calculation
  - Fixed: direct um^2 -> dbu^2 conversion without sqrt roundtrip

- [ ] **P2** GF180MCU min_spacing DRM verification needed
  - `pdks/gf180mcu.toml:29`: TOML says 0.46um, one source claims DRM 14.6.2 specifies 0.44um
  - 0.02um difference matters at 180nm - needs verification against official DRM document
  - Files: `pdks/gf180mcu.toml`

- [x] **P2** Density window silently skipped when pitch > window
  - Fixed: added tracing::warn when window_px == 0

- [x] **P2** K-means convergence feedback
  - Fixed: debug log on convergence, warn on max iterations reached

- [x] **P2** PDK layer number collision validation
  - Fixed: validate() checks artwork_layer vs artwork_layer_alt GDS layer/datatype collision

- [x] **P2** README "vectorize" language leftover
  - Fixed: changed to "Styles that convert well"

- [x] **P2** README roadmap marks density-aware generation as incomplete but it's implemented
  - Fixed: checked the roadmap box

- [x] **P2** CLI help text lists only 3 of 6 built-in PDKs
  - Fixed: updated to list all 6 PDKs + custom TOML

- [ ] **P2** No dry-run/validate subcommand
  - Can't preview DRC results, polygon count, or check settings without writing GDS
  - Useful for iterating on threshold/strategy/size before committing to expensive operations
  - Fix: add `--dry-run` flag that runs full pipeline but skips GDS write, or a `validate` subcommand
  - Files: `src/main.rs`

- [x] **P2** README "What makes it fast" section outdated (missing SAT mention)
  - Fixed: added SAT-based density checking mention

- [ ] **P2** DRC check is opt-in, should be default
  - `--check-drc` must be explicitly passed; default workflow produces GDS with no DRC validation
  - Combined with touching mode bugs, users can silently produce non-DRC-clean output
  - Fix: make DRC checking default-on with `--no-check-drc` to opt out
  - Files: `src/main.rs:122`

- [x] **P2** K-means empty cluster not reinitialized
  - Fixed: empty clusters reinitialized to farthest pixel from nearest centroid

- [x] **P2** Min-area filtering silently removes small features
  - Fixed: tracing::warn when min_area filter removes rects, with count and percentage

- [ ] **P2** GDS layer/datatype range not validated against GDS spec
  - `pdk.rs`: no validation that gds_layer is in [0, 32767] or gds_datatype is in [0, 255]
  - Custom PDK with out-of-range values would produce silently corrupted GDS binary
  - Fix: add range checks in PdkConfig::validate() or load()
  - Files: `src/pdk.rs`

- [x] **P2** Invalid --threshold values silently default to 128
  - Fixed: parse_threshold now returns Result, errors on invalid values

- [x] **P2** No unit tests for edge cases (empty image, all-white, single pixel)
  - Fixed: added in tests/end_to_end.rs - empty_bitmap, single_pixel, all_strategies tests

- [x] **P2** README preamble "ready to be fabricated" overpromises vs disclaimer
  - Fixed: softened to "for top-metal chip artwork. Verify with your foundry DRC tools before tapeout."

- [ ] **P2** SKY130 met4 alt-layer density_window_um=50 may be too small
  - `pdks/sky130.toml:48`: met4 density window is 50um vs met5's 700um - a 14x difference
  - Real SKY130 copper density checks typically use larger windows (200-500um)
  - May cause artwork that passes local density but fails foundry-level density verification
  - Fix: verify against SKY130 PDK documentation and update if needed
  - Files: `pdks/sky130.toml`

## Completed

- [x] Make touching mode the default, add `--separated` opt-out flag
- [x] Logo + example gallery + GitHub Pages preview gallery (`58d37bf`)
- [x] README header redesign with logo and poem
- [x] Generate SVG/HTML outputs for all input images across PDKs
- [x] Fix `count_on_in_window` build error in `src/artwork.rs`
