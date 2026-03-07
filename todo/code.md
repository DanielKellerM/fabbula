# fabbula - Rust Best Practices Audit

> Tracking file for code quality improvements. Each item has a status checkbox, priority, and context.

## Legend

- `[ ]` Open - not started
- `[~]` In progress
- `[x]` Done
- Priority: **P0** (blocker) / **P1** (high) / **P2** (nice-to-have)

---

## Type Safety

- [x] **P1** Create `Dbu(i32)` newtype for database units
  - Implemented: `Dbu(pub i32)` newtype with full operator overloading
  - `Point` and `Rect` fields are now `Dbu`, `Rect::new` accepts `impl Into<Dbu>`
  - `um_to_dbu()` returns `Dbu`, `.0` extraction at FFI boundaries (gds21, rstar)
  - Files: all src/ modules updated, zero-cost abstraction (newtype compiles away)

- [x] **P1** Create PDK name enum instead of stringly-typed `&str`
  - Added `BuiltinPdk` enum with `FromStr`, `Display`, `name()`, `toml_content()`, `all()`
  - `PdkConfig::builtin()` and `list_builtins()` use the enum internally
  - Files: `src/pdk.rs`, `src/main.rs`

- [x] **P1** Normalize coordinate system handling
  - Added `image_y_to_layout()` helper in polygon.rs, replaced 3 ad-hoc Y-flip expressions
  - Files: `src/polygon.rs`

- [x] **P2** Replace `bool` parameters with enums
  - `PixelPlacement::Touching/Separated` in polygon.rs/generation.rs/main.rs
  - `DitherMode::Off/FloydSteinberg` in artwork.rs/main.rs
  - `LayerVariant::Primary/Alternative` in pdk.rs
  - Files: `src/polygon.rs`, `src/artwork.rs`, `src/pdk.rs`, `src/main.rs`

- [x] **P2** Add `Point` newtype for `(i32, i32)` location tuples
  - Added `Point { x: i32, y: i32 }` in polygon.rs with `Display` impl
  - `DrcViolation.location` now uses `Point` instead of `(i32, i32)`
  - Re-exported in lib.rs
  - Files: `src/polygon.rs`, `src/drc.rs`, `src/lib.rs`

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

- [x] **P2** Pre-allocate density enforcement vectors
  - Pre-allocated `violations` Vec with capacity estimate, hoisted `candidates` Vec with `with_capacity(win*win)` and `.clear()`
  - Files: `src/artwork.rs`

- [x] **P2** Parallelize k-means clustering in `color.rs`
  - Parallel pixel assignment and centroid accumulation via `par_iter`/`par_chunks` + reduce (above 100K pixels)
  - Files: `src/color.rs`

- [x] **P2** Avoid full pixel Vec allocation in palette extraction
  - Subsample pixels for k-means training (every Nth pixel, target 50K samples)
  - Final assignment done directly from image iterator for small images, parallel for large
  - Files: `src/color.rs`

## Idiomatic Rust

- [x] **P2** Use `split_once()` instead of `split().collect()` + length check
  - Updated `parse_size_um` and `parse_layer_spec` in `main.rs`

- [x] **P2** Use `reduce()` in `bounding_box()`
  - Updated both `bounding_box()` and `bounding_box_refs()` in `polygon.rs`

- [x] **P2** Move `most_conservative_drc()` to `DrcRules` impl
  - Added `DrcRules::most_conservative(rules: &[DrcRules]) -> Self`
  - `main.rs` thin wrapper calls the method
  - Files: `src/pdk.rs`, `src/main.rs`

- [x] **P2** Move `validate_drc_rules()` to `DrcRules` impl
  - Added `DrcRules::validate(&self, section: &str) -> Result<()>`
  - `PdkConfig::validate()` calls `self.drc.validate("drc")`
  - Files: `src/pdk.rs`

## Code Organization

- [x] **P2** Extract generation logic from `main.rs`
  - Moved `generate_with_density_loop()`, `density_prepass()`, `generate_layer_polygons()` to `src/generation.rs`
  - Added `pub mod generation;` to lib.rs
  - Files: `src/generation.rs`, `src/main.rs`, `src/lib.rs`

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

- [x] **P2** Add tests for parallel merge strategy paths
  - Added `test_greedy_merge_parallel_path` and `test_histogram_merge_parallel_path` with 1000x1000 bitmaps
  - Verifies both serial and parallel code paths (>= 800K pixels, height > 256)
  - Files: `src/polygon.rs`

- [x] **P2** Add error path tests for file I/O
  - Added: `test_load_gds_nonexistent_path`, `test_load_gds_invalid_content`,
    `test_read_existing_metal_nonexistent`, `test_read_existing_metal_missing_cell`,
    `test_write_and_read_back_empty_gds`
  - Files: `src/gdsio.rs`

- [x] **P2** Add DRC early-exit edge case tests
  - Added: `test_capped_with_zero`, `test_capped_with_one`, `test_capped_exceeding_total`,
    `test_density_only_empty_rects`

- [x] **P2** Add property-based tests
  - Added proptest module with 5 properties: `rect_new_normalizes`, `bounding_box_contains_all`,
    `pixel_rects_covers_all_on_pixels`, `row_merge_preserves_pixel_count`,
    `greedy_merge_preserves_area`, `histogram_merge_preserves_area`
  - Files: `src/polygon.rs`, `Cargo.toml`
