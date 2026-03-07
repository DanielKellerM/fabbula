# fabbula - Rust Best Practices Audit

> Tracking file for code quality improvements. Each item has a status checkbox, priority, and context.

## Legend

- `[ ]` Open - not started
- `[~]` In progress
- `[x]` Done
- Priority: **P0** (blocker) / **P1** (high) / **P2** (nice-to-have)

---

## Type Safety

- [ ] **P1** Create `Dbu(i32)` newtype for database units
  - DBU values (nm) and pixels/micrometers are all raw `i32`/`f64` with no type-level distinction
  - `polygon.rs`: `um_to_dbu()` returns plain `i32`, easy to mix with pixel coords
  - `Rect` uses bare `i32` for x0/y0/x1/y1 - should be `Dbu`
  - Also consider `Micrometers(f64)` for PDK-facing values
  - Files: `src/polygon.rs`, `src/drc.rs`, `src/pdk.rs`

- [ ] **P1** Create PDK name enum instead of stringly-typed `&str`
  - PDK names are raw strings throughout (`main.rs:73`, `pdk.rs:155-166`)
  - No validation until runtime dispatch in `PdkConfig::load()`
  - An enum (`Sky130`, `IhpSg13g2`, `Gf180mcu`, etc.) catches typos at compile time
  - Files: `src/pdk.rs`, `src/main.rs`

- [ ] **P1** Normalize coordinate system handling
  - Y-axis flipping happens ad-hoc in multiple places with no consistent abstraction
  - `polygon.rs:184`: `bitmap.height - 1 - y` (artwork to layout)
  - `tiles.rs:104`: `bb.y1 - rect.y1` (layout to screen)
  - Risk of subtle bugs when adding new output formats or features
  - Consider helper functions or a `CoordSystem` enum with conversion methods
  - Files: `src/polygon.rs`, `src/preview.rs`, `src/tiles.rs`

- [ ] **P2** Replace `bool` parameters with enums
  - `touching: bool` in `main.rs:304,394,482,505` and `polygon.rs:108` - should be `PixelPlacement::Touching/Separated`
  - `dither: bool` in `artwork.rs:275` - should be `DitherMode::Enabled/Disabled`
  - `use_alt: bool` in `pdk.rs:188` - should be `DrcLayerVariant::Primary/Alternative`
  - Enums are self-documenting and prevent argument-order bugs
  - Files: `src/main.rs`, `src/polygon.rs`, `src/artwork.rs`, `src/pdk.rs`

- [ ] **P2** Add `Point` newtype for `(i32, i32)` location tuples
  - `DrcViolation.location` is `(i32, i32)` - no semantic meaning
  - `Transform.apply()` returns `(i32, i32)` - same issue
  - A `Point { x: i32, y: i32 }` struct would improve readability
  - Files: `src/drc.rs`, `src/gdsio.rs`

## Error Handling

- [x] **P1** Replace PID-based temp file with `tempfile` crate
  - Already using `tempfile::NamedTempFile` in `gdsio.rs`

- [x] **P1** Validate AREF instance count before flattening
  - Added `MAX_AREF_INSTANCES` (100,000) limit with warning in `gdsio.rs`

- [x] **P2** Create helper for gds21 error conversion
  - Added `gds_err()` helper in `gdsio.rs`, all call sites use it

- [x] **P2** Add context to PDK validation error messages
  - `validate_drc_rules()` already takes a `section` parameter and includes it in errors

- [x] **P2** Make `parse_threshold` and `parse_hex_color` return errors instead of silent defaults
  - `parse_threshold` already returns `Result` with clear error messages
  - `parse_hex_color` now logs a warning on invalid input

- [x] **P2** Warn on zero-width GDS paths
  - Added `tracing::warn!` in `gdsio.rs` for zero/missing path width

## API Design

- [x] **P2** Add `#[must_use]` to critical public functions
  - Added to `generate_polygons()`, `check_drc()`, `check_drc_capped()`, `check_density_only()`, `load_artwork()`

- [x] **P2** Curate public API with `pub use` re-exports in `lib.rs`
  - Re-exported `Rect`, `ArtworkBitmap`, `ThresholdMode`, `DrcRule`, `DrcViolation`, `PdkConfig`, `DrcRules`, `PolygonStrategy`

- [x] **P2** Add `#[non_exhaustive]` to public enums
  - Added to `ThresholdMode`, `PolygonStrategy`, `DrcRule`, `ColorMode`

- [x] **P2** Add `Display` impls for diagnostic enums
  - Added `Display` for `ThresholdMode` and `PolygonStrategy`
  - `DrcRule` already had `Display`

- [x] **P2** Extract constants for magic numbers
  - Extracted `POLYGON_OPACITY` in `tiles.rs`

## Performance

- [x] **P2** Add `#[inline]` to `Rect::new()`
  - Added `#[inline]` attribute

- [ ] **P2** Pre-allocate density enforcement vectors
  - `artwork.rs:584`: `violations` Vec grows during window scan - could estimate capacity
  - `artwork.rs:617`: `candidates` Vec inside inner loop - could pre-allocate `win*win`
  - Reduces reallocations in density enforcement, estimated 5-10% speedup for dense images
  - Files: `src/artwork.rs`

- [ ] **P2** Parallelize k-means clustering in `color.rs`
  - K-means centroid update loop (`color.rs:271`) doesn't use rayon
  - Polygon generation and DRC already use `par_iter()` extensively
  - Large images in palette mode could benefit from parallel pixel assignment
  - Files: `src/color.rs`

- [ ] **P2** Avoid full pixel Vec allocation in palette extraction
  - `color.rs:150-153`: collects all pixels into `Vec<[f32; 3]>` before k-means
  - For large images this is a significant allocation
  - Consider iterator-based k-means or chunked processing
  - Files: `src/color.rs`

## Idiomatic Rust

- [x] **P2** Use `split_once()` instead of `split().collect()` + length check
  - Updated `parse_size_um` and `parse_layer_spec` in `main.rs`

- [x] **P2** Use `reduce()` in `bounding_box()`
  - Updated both `bounding_box()` and `bounding_box_refs()` in `polygon.rs`

- [ ] **P2** Move `most_conservative_drc()` to `DrcRules` impl
  - Currently a free function in `main.rs` operating on `&[ArtworkLayerProfile]`
  - More idiomatic as `DrcRules::most_conservative(rules: &[DrcRules]) -> Self`
  - Also avoids unnecessary clone of `profiles[0].drc`
  - Files: `src/main.rs`, `src/pdk.rs` or `src/drc.rs`

- [ ] **P2** Move `validate_drc_rules()` to `DrcRules` impl
  - `pdk.rs:234-288`: standalone function that validates a `&DrcRules`
  - More idiomatic as `impl DrcRules { pub fn validate(&self) -> Result<()> }`
  - Files: `src/pdk.rs`

## Code Organization

- [ ] **P2** Extract generation logic from `main.rs`
  - `generate_with_density_loop()` (~90 lines), `density_prepass()`, `generate_layer_polygons()` contain business logic
  - These are untestable without the CLI - should move to `src/generation.rs` or into existing modules
  - `main.rs` is 1,022 lines; extracting would improve testability and readability
  - Files: `src/main.rs`, `src/lib.rs`

- [x] **P2** Standardize derive macro ordering
  - Fixed `Transform` in `gdsio.rs` from `Clone, Debug` to `Debug, Clone`

## Testing

- [x] **P1** Add boundary condition tests for DRC
  - Added tests for exactly-at-limit values: `test_width_exactly_at_min`, `test_width_one_below_min`,
    `test_spacing_exactly_at_min`, `test_spacing_one_below_min`, `test_area_exactly_at_min`,
    `test_area_below_min`, `test_max_width_exactly_at_max`, `test_max_width_one_above`

- [x] **P1** Add empty/zero-size input tests
  - Added: `test_bounding_box_empty`, `test_bounding_box_single`, `test_bounding_box_refs_empty`,
    `test_single_pixel_bitmap`, `test_all_off_bitmap`, `test_single_row_bitmap`,
    `test_single_column_bitmap`, `test_rect_zero_area`, `test_drc_empty_rects`, `test_drc_single_rect`

- [ ] **P2** Add tests for parallel merge strategy paths
  - `greedy_merge_parallel_strips` only triggers at >= 800,000 pixels AND height > 256
  - `histogram_merge_parallel_strips` has same threshold
  - Current tests use small bitmaps (128x128) - parallel paths are never exercised
  - Add ~1000x1000 bitmap tests to cover parallel code paths
  - Files: `src/polygon.rs`

- [ ] **P2** Add error path tests for file I/O
  - No tests for corrupted GDS files, truncated .gz input, or unwritable output paths
  - `load_gds` with a non-GDS file should return a clear error
  - Files: `src/gdsio.rs`

- [x] **P2** Add DRC early-exit edge case tests
  - Added: `test_capped_with_zero`, `test_capped_with_one`, `test_capped_exceeding_total`,
    `test_density_only_empty_rects`

- [ ] **P2** Add property-based tests
  - All tests are hand-written unit/integration examples
  - Bitmap manipulations, SAT, histogram merge are good candidates for proptest/quickcheck
  - Could catch subtle edge cases in coordinate arithmetic and merge algorithms
  - Files: `src/polygon.rs`, `src/artwork.rs`, `Cargo.toml`
