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

- [ ] **P1** Replace PID-based temp file with `tempfile` crate
  - `gdsio.rs:29-44`: uses `format!("fabbula_decompress_{}.gds", std::process::id())`
  - PID naming can collide; cleanup uses `let _ = remove_file()` which silently ignores errors
  - `tempfile::NamedTempFile` handles cleanup on drop and avoids collisions
  - Files: `src/gdsio.rs`, `Cargo.toml`

- [ ] **P1** Validate AREF instance count before flattening
  - `gdsio.rs:426-442`: iterates `rows * cols` instances with no upper bound check
  - Large AREFs (millions of instances) could exhaust memory or hang
  - Add a reasonable limit (e.g. 100,000 instances) with a warning
  - Files: `src/gdsio.rs`

- [ ] **P2** Create helper for gds21 error conversion
  - `gds21::GdsError` doesn't impl `std::error::Error`, so every call site uses `map_err(|e| anyhow!(...))`
  - Repeated in `gdsio.rs` at lines 37-48, 113-114, 178-179
  - A `fn gds_err(e: gds21::GdsError, context: &str) -> anyhow::Error` would centralize the quirk
  - Files: `src/gdsio.rs`

- [ ] **P2** Add context to PDK validation error messages
  - `pdk.rs:244-287`: `ensure!` messages say "Manufacturing grid must be positive" but don't name which PDK
  - Include `self.pdk.name` in error messages so users know which config file is broken
  - Files: `src/pdk.rs`

- [ ] **P2** Make `parse_threshold` and `parse_hex_color` return errors instead of silent defaults
  - `main.rs:291-300`: invalid threshold string silently defaults to `Luminance(128)`
  - `tiles.rs:50-60`: invalid hex color silently defaults to gray `[192,192,192]`
  - Both should return `Result` or at least log a warning
  - Files: `src/main.rs`, `src/tiles.rs`

- [ ] **P2** Warn on zero-width GDS paths
  - `gdsio.rs:365`: `p.width.unwrap_or(0)` silently treats missing width as zero
  - Creates zero-width geometry with no user feedback
  - Add `tracing::warn!` when path has no width
  - Files: `src/gdsio.rs`

## API Design

- [ ] **P2** Add `#[must_use]` to critical public functions
  - `generate_polygons()`, `check_drc()`, `load_artwork()` return important results
  - Callers silently discarding these would be a bug
  - Files: `src/polygon.rs`, `src/drc.rs`, `src/artwork.rs`

- [ ] **P2** Curate public API with `pub use` re-exports in `lib.rs`
  - All modules are `pub mod` with no organized re-exports
  - Key types like `Rect`, `ArtworkBitmap`, `DrcViolation`, `PdkConfig` should be re-exported
  - Makes the library API surface explicit for downstream users
  - Files: `src/lib.rs`

- [ ] **P2** Add `#[non_exhaustive]` to public enums
  - `ThresholdMode`, `PolygonStrategy`, `DrcRule` may grow new variants
  - Without `#[non_exhaustive]`, adding variants is a breaking change for library users
  - Files: `src/artwork.rs`, `src/polygon.rs`, `src/drc.rs`

- [ ] **P2** Add `Display` impls for diagnostic enums
  - `ThresholdMode`, `PolygonStrategy`, `ColorMode` lack `Display`
  - Currently rely on `Debug` formatting in logs, which is less readable
  - Files: `src/artwork.rs`, `src/polygon.rs`, `src/color.rs`

- [ ] **P2** Extract constants for magic numbers
  - `preview.rs:228`: opacity value `217` (0.85) inlined without a named constant
  - Other scattered literals in preview/tile generation code
  - Extract as `const POLYGON_OPACITY: u8 = 217;` etc.
  - Files: `src/preview.rs`, `src/tiles.rs`

## Performance

- [ ] **P2** Add `#[inline]` to `Rect::new()`
  - Called O(polygon count) times in all merge strategies
  - `width()`, `height()`, `area()` are already `#[inline]` but `new()` is not
  - Files: `src/polygon.rs`

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

- [ ] **P2** Use `split_once()` instead of `split().collect()` + length check
  - `main.rs:367-380` (`parse_layer_spec`): splits on '/', collects to Vec, checks len == 2
  - `main.rs:304-315` (`parse_size_um`): same pattern
  - `split_once('/')` is cleaner - returns `Option<(&str, &str)>`, avoids Vec allocation
  - Files: `src/main.rs`

- [ ] **P2** Use `reduce()` in `bounding_box()`
  - `polygon.rs:747-755`: manually takes `first()` then `fold()` over `rects[1..]`
  - `rects.iter().copied().reduce(|bb, r| ...)` is equivalent and more idiomatic
  - Files: `src/polygon.rs`

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

- [ ] **P2** Standardize derive macro ordering
  - Most structs use `Debug, Clone, Copy, PartialEq, Eq` order
  - `gdsio.rs:218`: `Transform` uses `Clone, Debug` (non-standard order)
  - Minor but inconsistency is visible when scanning the codebase
  - Files: `src/gdsio.rs`

## Testing

- [ ] **P1** Add boundary condition tests for DRC
  - No tests for values exactly at limits (min_width, min_spacing, min_area, density_max)
  - Current tests use values that clearly violate - boundaries are where bugs hide
  - Test: rect at exactly `min_width` should pass; at `min_width - 1` should fail
  - Files: `src/drc.rs`

- [ ] **P1** Add empty/zero-size input tests
  - `bounding_box([])` returns `None` but no test covers this
  - `write_svg_multi` with empty rects falls back to default bounds - untested
  - Single-pixel and degenerate (zero-width/height) rectangles untested
  - Files: `src/polygon.rs`, `src/preview.rs`

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

- [ ] **P2** Add DRC early-exit edge case tests
  - `check_drc_capped` with cap=0 and cap=1 untested
  - Cap exceeding total violations untested (should not early exit)
  - Files: `src/drc.rs`

- [ ] **P2** Add property-based tests
  - All tests are hand-written unit/integration examples
  - Bitmap manipulations, SAT, histogram merge are good candidates for proptest/quickcheck
  - Could catch subtle edge cases in coordinate arithmetic and merge algorithms
  - Files: `src/polygon.rs`, `src/artwork.rs`, `Cargo.toml`
