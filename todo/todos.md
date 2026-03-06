# fabbula - TODO Tracker

> Tracking file for Claude Code. Each task has a status checkbox, priority, and context to enable autonomous work.

## Legend

- `[ ]` Open - not started
- `[~]` In progress
- `[x]` Done
- Priority: **P0** (blocker) / **P1** (high) / **P2** (nice-to-have)

---

## In Progress

- [~] **P0** Audit fixes - competitive analysis and usability review
  - A1: Fix wide_metal_spacing in pitch calculation (DRC-by-construction bug)
  - A2: Fix README to describe actual pipeline (not fake vectorization)
  - B1: Add `--size-um` CLI parameter for physical dimension specification
  - B2: Add artwork bounds reporting after generation/merge
  - B5: Better error messages with available cell list
  - B4: Compressed GDS support (.gds.gz)
  - B3: Die boundary awareness warning
  - A3: Verify FreePDK45 wide_metal threshold
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

## Features

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

- [ ] **P2** Support compressed GDS input (.gds.gz, .gds.tar.gz)
  - Real chip GDS files are commonly distributed gzip-compressed
  - Add transparent decompression in `read_existing_metal()` and `merge_into_gds_multi()`
  - Detect by file extension (.gz) and decompress to temp file or stream via `flate2` crate
  - Also handle `.tar.gz` archives (extract first .gds file)
  - Files: `src/gdsio.rs`, `Cargo.toml` (add `flate2` dep)

## Performance / Profiling

- [ ] **P2** Die-scale benchmarks for profiling suite
  - Separate benchmark group (`cargo bench --bench bench_die_scale`), not in default `cargo bench`
  - Option 1: Checkerboard 17812x17812 - bump `gen_test_image.rs` size. Stresses greedy-merge + GDS write without density enforcement dominating
  - Option 2: Noise/mixed pattern ~40% fill at 17812x17812 - exercises density enforcement realistically
  - Files: `profiling/gen_test_image.rs`, `profiling/bench_die_scale.rs`

## Infrastructure

- [ ] **P2** Set up GitHub Pages deployment
  - Enable Pages in repo settings: Source = "Deploy from branch", Branch = `main`, Folder = `/docs`
  - `docs/index.html` and `docs/previews/` already committed

## Completed

- [x] Logo + example gallery + GitHub Pages preview gallery (`58d37bf`)
- [x] README header redesign with logo and poem
- [x] Generate SVG/HTML outputs for all input images across PDKs
- [x] Fix `count_on_in_window` build error in `src/artwork.rs`
