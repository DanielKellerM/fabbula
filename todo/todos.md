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

- [x] **P2** Smart auto-polarity and threshold improvements
  - Fixed: added `--threshold auto` mode (Otsu + auto-polarity)
  - Auto-inverts when >50% pixels are metal (bright subject on dark background)
  - Remaining ideas (adaptive thresholding, edge detection, histogram analysis) deferred
  - Files: `src/artwork.rs`, `src/main.rs`

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

- [x] **P1** CLI regression testing with snapshot testing
  - tests/cli_regression.rs: 34 tests using insta snapshots and structural assertions
  - Deterministic 16x16 checkerboard test image (no binaries in repo)
  - Snapshots for: list-pdks, show-pdk (x6), SVG (x6), LEF (x6), GDS stats (x6)
  - Strategy comparison, DRC check, separated mode, invert, dither, otsu, error cases
  - CI: added MSRV (1.93) job and cargo-machete unused dependency check

### P2 - Improvements

- [x] **P2** No rotation/flip support
  - Fixed: added `--rotate` (0/90/180/270) and `--flip` (horizontal/vertical) CLI flags
  - ArtworkBitmap rotate(), flip_horizontal(), flip_vertical() methods
  - Applied in both generate and merge, all color modes
  - Files: `src/artwork.rs`, `src/main.rs`

- [x] **P2** Single-layer artwork per invocation
  - Addressed: color modes (channel/palette) already generate multi-layer output
  - Arbitrary layer targeting remains unsupported but is a niche use case
  - Users can run fabbula twice with different PDK configs for fully custom layer targets

- [x] **P2** No CI matrix for cross-platform or MSRV testing
  - Added macOS runner to build matrix (ubuntu-latest + macos-latest)
  - Also added coverage job (cargo-llvm-cov, 70% threshold) and cargo audit job
  - Files: `.github/workflows/ci.yml`

- [x] **P2** No `cargo audit` in CI
  - Added cargo-audit job to CI pipeline via taiki-e/install-action
  - Files: `.github/workflows/ci.yml`

- [x] **P2** max_width enforced by splitting, not slotting
  - Documented: DRC coverage table in README notes "Partial" for max_width
  - Splitting keeps all rects below max_width, which satisfies width rules
  - Real slotting (holes in wide metal) not needed since fabbula generates discrete rects, not wide fills
  - Files: `README.md`

- [x] **P2** Density enforcement can fail silently
  - Fixed: non-convergence now returns an error instead of a warning
  - `--force` flag overrides and allows continuing despite density violations
  - Added to both generate and merge commands
  - Files: `src/generation.rs`, `src/main.rs`

- [x] **P2** Document which DRC rules are and aren't checked
  - Fixed: added "DRC coverage" table to README under Tapeout Integration
  - Lists enforced rules (width, spacing, wide-metal, max-width, area, density) with method
  - Lists inapplicable rules (enclosure, antenna, acute angle, off-grid, same-net, via)
  - Files: `README.md`

- [x] **P2** Clarify LEF output limitations in README
  - Fixed: LEF now emits actual geometry, no longer bounding-box only

- [x] **P2** min_area sqrt rounding - use direct dbu^2 calculation
  - Fixed: direct um^2 -> dbu^2 conversion without sqrt roundtrip

- [x] **P2** GF180MCU min_spacing DRM verification needed
  - Verified: 0.46um min_spacing per DRM 14.6.2 (distinct from 0.44um min_width per DRM 14.6.1)
  - Added explicit DRM section references as comments in TOML
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

- [x] **P2** No dry-run/validate subcommand
  - Fixed: added `--dry-run` flag to generate command (runs full pipeline, skips GDS write)
  - Reports polygon count and layer count without writing files
  - Files: `src/main.rs`

- [x] **P2** README "What makes it fast" section outdated (missing SAT mention)
  - Fixed: added SAT-based density checking mention

- [x] **P2** DRC check is opt-in, should be default
  - Fixed: DRC checking now runs by default; `--no-check-drc` flag to opt out
  - Files: `src/main.rs`

- [x] **P2** K-means empty cluster not reinitialized
  - Fixed: empty clusters reinitialized to farthest pixel from nearest centroid

- [x] **P2** Min-area filtering silently removes small features
  - Fixed: tracing::warn when min_area filter removes rects, with count and percentage

- [x] **P2** GDS layer/datatype range not validated against GDS spec
  - Fixed: validate_gds_range() checks layer >= 0 and datatype >= 0 for all layer configs
  - Covers artwork_layer, artwork_layer_alt, artwork_layers[], and metal_stack[]
  - Files: `src/pdk.rs`

- [x] **P2** Invalid --threshold values silently default to 128
  - Fixed: parse_threshold now returns Result, errors on invalid values

- [x] **P2** No unit tests for edge cases (empty image, all-white, single pixel)
  - Fixed: added in tests/end_to_end.rs - empty_bitmap, single_pixel, all_strategies tests

- [x] **P2** README preamble "ready to be fabricated" overpromises vs disclaimer
  - Fixed: softened to "for top-metal chip artwork. Verify with your foundry DRC tools before tapeout."

- [x] **P2** SKY130 met4 alt-layer density_window_um=50 may be too small
  - Fixed: increased met4 density_window_um from 50 to 500 (consistent with GF180MCU copper layers)
  - 50um was from local gradient check spec; 500um is more appropriate for max density enforcement
  - Files: `pdks/sky130.toml`

## Open-Source Audit (2026-03-07)

> Pre-release audit. Findings organized by severity. Items already addressed in prior audits are marked [x] above.

### P0 - Correctness

- [x] **P0** min_area DRC check still uses sqrt roundtrip
  - Fixed: direct um^2 -> dbu^2 conversion without sqrt roundtrip (matching polygon.rs)
  - Files: `src/drc.rs`

- [x] **P0** Merge command "top cell" heuristic picks last struct, not actual top cell
  - Fixed: walks SREF/AREF refs to find the unreferenced cell (true top cell)
  - Falls back to last struct if no unique unreferenced cell found
  - Files: `src/gdsio.rs`

- [x] **P0** No DRC check in merge command
  - Fixed: added --no-check-drc flag and default-on DRC checking to merge command
  - Files: `src/main.rs`

### P1 - Robustness

- [x] **P1** GDS output units use gds21 defaults, not PDK-specific values
  - Fixed: write_gds_multi now sets GdsUnits from db_units_per_um
  - Files: `src/gdsio.rs`, `src/main.rs`

- [x] **P1** ASAP7 4x scaling not documented or handled
  - Fixed: added prominent 4x scaling warning in asap7.toml header
  - Files: `pdks/asap7.toml`

- [x] **P1** GF180MCU Metal5 missing wide_metal rules
  - Fixed: added wide_metal_threshold=10.0 and wide_metal_spacing=0.60 per DRM 14.6.4
  - Files: `pdks/gf180mcu.toml`

- [x] **P1** generation.rs has 52% test coverage (lowest in codebase)
  - Fixed: added 5 unit tests covering generate_layer_polygons and density_prepass
  - Coverage improved from 52% to 78%
  - Files: `src/generation.rs`

- [ ] **P1** Generate and Merge share ~80% duplicated logic in main.rs
  - Deferred: large refactor with high risk of regressions; both paths work correctly
  - The DRC and density enforcement are now consistent between both commands
  - Files: `src/main.rs`

- [x] **P1** No off-grid validation after merge offset
  - Fixed: added manufacturing grid alignment warning for offset-x/offset-y
  - Files: `src/main.rs`

- [ ] **P1** Touching mode DRC-by-construction lacks exhaustive test coverage
  - Deferred: existing end_to_end tests cover all PDKs x all strategies in touching mode
  - The touching_mode_all_pdks_all_strategies test verifies DRC-clean output
  - Proptest would add value but is a significant new dependency
  - Files: `tests/end_to_end.rs`

### P2 - Quality / Credibility

- [x] **P2** SVG preview scale hardcoded to 0.01
  - Fixed: adaptive scale computed from bounding box (targets 1024px max dimension)
  - Files: `src/main.rs`

- [x] **P2** LEF output missing SITE and USE declarations
  - Fixed: added SITE core declaration to LEF output
  - Files: `src/lef.rs`

- [x] **P2** No Windows CI testing
  - Fixed: added windows-latest to build matrix
  - Files: `.github/workflows/ci.yml`

- [x] **P2** GDS library name always hardcoded to "fabbula"
  - Fixed: added --library-name flag to Generate and Merge commands
  - Files: `src/gdsio.rs`, `src/main.rs`

- [x] **P2** No GDS TEXT/label element support
  - Documented as limitation in gdsio.rs module docs
  - TEXT/label support is out of scope; users can add labels via EDA tools
  - Files: `src/gdsio.rs`

- [x] **P2** fabbula2 (imaginary 2nm PDK) in list-pdks hurts credibility
  - Fixed: added "(virtual)" tag for fabbula2, freepdk45, and asap7 in list-pdks output
  - Files: `src/main.rs`

- [x] **P2** No DEF output for OpenROAD/OpenLane integration
  - Documented as limitation in gdsio.rs module docs
  - DEF output is a future enhancement; LEF is sufficient for most flows
  - Files: `src/gdsio.rs`

- [x] **P2** No exclusion support for non-metal layers (pad openings, RDL, bumps)
  - Already addressed: --exclusion-layer flag allows specifying arbitrary GDS layer/datatype
  - Users can exclude any layer (pad openings, seal rings) by specifying the layer number
  - Files: `src/main.rs`

- [x] **P1** CLI regression tests don't assert DRC clean, only no-crash
  - Fixed: added VIOLATION assertion to generate_check_drc_all_pdks test
  - Files: `tests/cli_regression.rs`

- [x] **P1** IHP SG13G2 min_area=0.0 may be incorrect
  - Updated comment to clarify TopMetal2 specifically and suggest verifying with DRC deck
  - The value 0.0 disables min-area checking (conservative in terms of not over-constraining)
  - Files: `pdks/ihp_sg13g2.toml`

- [x] **P1** Wide-metal spacing doesn't distinguish wide-to-narrow vs wide-to-wide
  - Documented limitation in drc.rs code comments
  - Single-threshold approach is conservative (applies wider spacing to both cases)
  - Files: `src/drc.rs`

- [x] **P2** Density grid origin tied to bbox, not chip/manufacturing grid
  - Documented limitation in drc.rs code comments
  - For standalone artwork generation, bbox-aligned grid is appropriate
  - Files: `src/drc.rs`

- [x] **P2** docs/PIPELINE.md describes unimplemented SVG vectorization pipeline
  - Rewritten as future design document with clear "not yet implemented" status
  - Kept useful AI prompt engineering tips; removed stale dependency and CLI sections
  - Files: `docs/PIPELINE.md`

- [x] **P2** rect_spacing uses Manhattan approximation for diagonal gaps
  - Documented in drc.rs code comments; equivalent to Euclidean for grid-aligned output
  - Files: `src/drc.rs`

- [x] **P2** No real vectorization path (PNG to vector outlines)
  - Documented in PIPELINE.md as future enhancement
  - Raster-to-rectangle approach works well for chip artwork at typical resolutions

- [x] **P2** Disclaimer vs claims tension in README
  - Added explicit note above comparison table linking to disclaimer
  - Clarifies comparison reflects features, not silicon-proven status
  - Files: `README.md`

## Open-Source Audit (2026-03-07, Round 3)

> Critical review from the perspective of an expert semiconductor engineer asking:
> "Why do we need yet another tool? Does it really work? Is it usable for real tapeouts?"

### The Hard Questions

**"Why not just use ArtistIC?"**
ArtistIC has been used on real tapeouts. fabbula has not. That's the credibility gap.
Technically, ArtistIC uses 4x4 kernel pattern matching with 10 dithering primitives and
post-filters undersized polygons (< 2000nm). No polygon merging, no density enforcement,
no spacing checks. fabbula's DRC-by-construction approach is more rigorous, but ArtistIC
is silicon-proven on IHP SG13G2. ArtistIC handles exclusion via KLayout boolean subtraction.
Requires: KLayout, ImageMagick, Inkscape, Potrace, Python, gdspy. IHP SG13G2 only.

**"Does DRC-by-construction really work?"**
The core pitch-based approach is sound for width/spacing/area. But the claim has caveats that
aren't obvious enough to users:
- density_min is not enforced at all (documented but buried in a table)
- Wide-metal spacing uses a single threshold, not the multi-threshold model real PDKs use
- Max width is handled by splitting, not real slotting (holes in wide metal)
- No antenna rule checking (claimed N/A for floating metal, but some foundry decks flag it)
- Density window origin is bbox-aligned, not chip-grid-aligned like foundry tools

**"What about advanced nodes?"**
ASAP7 and fabbula2 are virtual PDKs - nobody is taping out artwork on 7nm or 2nm. The real
value proposition is SKY130, IHP SG13G2, and GF180MCU. Including virtual PDKs pads the count
but could hurt credibility with experts who know these aren't real.

### P1 - Credibility / Correctness

- [x] **P1** K-means palette quantization is non-deterministic
  - False positive: k-means uses deterministic init (darkest pixel first, then max-distance).
    No RNG involved. Results are reproducible for same input.

- [x] **P1** `write_gds()` hardcodes library name "fabbula" ignoring `--library-name`
  - Fixed: `write_gds()` now accepts `library_name` parameter
  - Files: `src/gdsio.rs`, `src/lib.rs`, `tests/end_to_end.rs`

- [x] **P1** Density prepass runs even when bitmap is already below density_max
  - Fixed: skip prepass when `bitmap.density() <= density_max`
  - Files: `src/generation.rs`

- [x] **P1** PDK validation missing wide_metal consistency checks
  - Fixed: validate wide_metal_threshold > min_width, threshold/spacing must be paired,
    density_window_um > 0 when density_max < 1.0. Added 4 unit tests.
  - Files: `src/pdk.rs`

- [x] **P1** README comparison table doesn't weight "silicon-proven" enough
  - Fixed: added "Tapeout proven" row to comparison table
  - Fixed: corrected ArtistIC description (was "tetromino fill", actually 4x4 kernel + size filter)
  - Fixed: ArtistIC DRC column now says "Post-filter (min size)" instead of "Tetromino fill"
  - Files: `README.md`

### P2 - Usability / Testing

- [x] **P2** Magic number 0.95 in density enforcement loop
  - Fixed: named constant DENSITY_MARGIN with rationale comment
  - Files: `src/generation.rs`

- [x] **P2** No tapeout checklist in documentation
  - Fixed: replaced "After generation" with 6-step tapeout checklist
  - Files: `README.md`

- [x] **P2** No test coverage for negative coordinates in DRC
  - Fixed: added 3 tests (clean, spacing violation, density) with negative-quadrant rects
  - Files: `src/drc.rs`

- [x] **P2** No degenerate PDK configuration tests
  - Fixed: added tests for density_max=1.0, equal width/spacing, large min_area,
    and 4 PDK validation edge case tests
  - Files: `src/drc.rs`, `src/pdk.rs`

- [x] **P2** SVG/HTML preview missing size warning for large outputs
  - Fixed: warn when rect count > 50k, suggesting --deep-zoom
  - Files: `src/preview.rs`

## Completed

- [x] Make touching mode the default, add `--separated` opt-out flag
- [x] Logo + example gallery + GitHub Pages preview gallery (`58d37bf`)
- [x] README header redesign with logo and poem
- [x] Generate SVG/HTML outputs for all input images across PDKs
- [x] Fix `count_on_in_window` build error in `src/artwork.rs`
