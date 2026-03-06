# fabbula - TODO Tracker

> Tracking file for Claude Code. Each task has a status checkbox, priority, and context to enable autonomous work.

## Legend

- `[ ]` Open - not started
- `[~]` In progress
- `[x]` Done
- Priority: **P0** (blocker) / **P1** (high) / **P2** (nice-to-have)

---

## In Progress

- [x] **P0** HFT-style performance optimization
  - Phase 0: Added density-only bench + profiling binary for flame graphs
  - Phase 1: GreedyMerge u16 runs, word-level bitops, direct bitset access
  - Phase 2: Parallel runs computation + strip-based parallel greedy merge
  - Phase 3: SAT-based density checking (replace R-tree queries)
  - Phase 4: DRC zero-alloc iterators, u32 IndexedRect, #[inline] on hot paths
  - Files: `src/polygon.rs`, `src/artwork.rs`, `src/drc.rs`, `profiling/bench_*.rs`

## Features

- [ ] **P1** Add color/multi-layer support from PDK layer maps
  - Parse layer color definitions from PDK TOML files
  - Map input image color channels to distinct GDS layers
  - Files: `src/pdk.rs` (layer config), `src/artwork.rs` (color extraction), `pdks/*.toml`
  - Depends on: deciding color-to-layer mapping strategy (palette? channel-based?)

- [ ] **P1** GDS import for merge workflow
  - Read existing GDS, extract top-metal geometry as exclusion zones
  - Currently `merge` subcommand exists but needs real GDS cell import
  - Files: `src/gdsio.rs` (read path), `src/main.rs` (merge subcommand)

## Infrastructure

- [ ] **P2** Set up GitHub Pages deployment
  - Enable Pages in repo settings: Source = "Deploy from branch", Branch = `main`, Folder = `/docs`
  - `docs/index.html` and `docs/previews/` already committed

## Completed

- [x] Logo + example gallery + GitHub Pages preview gallery (`58d37bf`)
- [x] README header redesign with logo and poem
- [x] Generate SVG/HTML outputs for all input images across PDKs
- [x] Fix `count_on_in_window` build error in `src/artwork.rs`
