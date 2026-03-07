# fabbula - TODO Tracker

> Tracking file for Claude Code. Each task has a status checkbox, priority, and context to enable autonomous work.

## Legend

- `[ ]` Open - not started
- `[~]` In progress
- `[x]` Done
- Priority: **P0** (blocker) / **P1** (high) / **P2** (nice-to-have)

---

## In Progress

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

## Completed

- [x] Make touching mode the default, add `--separated` opt-out flag
- [x] Logo + example gallery + GitHub Pages preview gallery (`58d37bf`)
- [x] README header redesign with logo and poem
- [x] Generate SVG/HTML outputs for all input images across PDKs
- [x] Fix `count_on_in_window` build error in `src/artwork.rs`
