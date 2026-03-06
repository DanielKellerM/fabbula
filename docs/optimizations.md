# fabbula Performance Optimization Log

This document tracks every optimization applied to fabbula, with rationale, technique, and measured impact. Organized chronologically by optimization phase.

---

## Phase 1: HFT-style Core Optimizations

**Commit:** `7a7f3fa`
**Files:** `src/polygon.rs`, `src/artwork.rs`, `src/drc.rs`

### 1.1 Packed Bitset Representation

**Problem:** `ArtworkBitmap` stored pixels as `Vec<bool>` - 1 byte per pixel, poor cache utilization.

**Fix:** Pack pixels into `Vec<u64>` bitset. Each u64 holds 64 pixels. Access via `(word >> bit) & 1`.

**Why it matters:** 8x memory reduction. A 4096x4096 bitmap drops from 16 MB to 2 MB, fitting in L2 cache. Every subsequent operation that scans the bitmap benefits from this.

### 1.2 u16 Horizontal Run Table

**Problem:** GreedyMerge computed horizontal runs as `Vec<u32>` - 4 bytes per pixel.

**Fix:** Switch to `Vec<u16>`. Bitmap widths are capped at 65535 (asserted via `debug_assert!`), so u16 suffices. Halves memory bandwidth for the runs table.

**Impact:** Reduces runs table from 4 bytes/pixel to 2 bytes/pixel. For a 4096x4096 image, saves 32 MB of memory traffic.

### 1.3 Word-level Bitset Operations

**Problem:** The `used` bitset in GreedyMerge was checked and set one bit at a time.

**Fix:** Two key functions operate on whole u64 words:

- `bulk_set_bits(used, start, len)`: Sets a range of bits using word-aligned masks. A single rectangle marking that previously required N individual bit-sets now touches at most `ceil(N/64) + 2` words.

- `effective_run_word_scan(used, start, raw)`: Scans for the first set bit in the `used` bitset within a range. Processes 64 bits per iteration using `trailing_zeros()`, which compiles to a single `tzcnt` instruction on x86.

### 1.4 Direct Bitset Access in Hot Loops

**Problem:** `bitmap.get(x, y)` does bounds checking (`x >= width || y >= height`) on every call, plus a multiply and divide for indexing.

**Fix:** In hot inner loops (run computation, greedy merge), access the raw `words` slice directly with pre-computed bit indices: `words[bit_idx / 64] & (1u64 << (bit_idx % 64))`. The compiler optimizes `/ 64` to `>> 6` and `% 64` to `& 63`.

### 1.5 Parallel Strip-based GreedyMerge

**Problem:** GreedyMerge is inherently sequential top-to-bottom (each pixel's rectangle depends on rows above).

**Fix:** Split the bitmap into horizontal strips of 256 rows. Each strip runs GreedyMerge independently in parallel via `rayon::par_iter`. Rectangles cannot span strip boundaries, so this trades a small increase in rectangle count for parallelism.

**Trade-off:** Strips that cross a large solid region produce two rectangles instead of one. In practice the count increase is negligible (<1%) because real artwork has features smaller than 256 pixels.

### 1.6 Parallel Horizontal Run Computation

**Problem:** Computing the runs table was serial even though each row is independent.

**Fix:** Use `par_chunks_mut` to compute runs for each row in parallel. Only activates above `PARALLEL_PIXEL_THRESHOLD` (800K pixels).

### 1.7 SAT-based Density Checking in DRC

**Problem:** DRC density check used per-window rectangle intersection queries.

**Fix:** Rasterize rectangle metal area into a grid (cell = half-window step), build a 2D prefix sum (summed area table), then query each window position in O(1). Total complexity: O(rects * avg_cells_per_rect + grid_size) instead of O(windows * rects).

### 1.8 DRC Zero-allocation Iterator Chains

**Problem:** Width/spacing checks allocated intermediate `Vec`s for violations.

**Fix:** Use `flat_map_iter` with `Option::into_iter().chain()` to produce violations lazily without intermediate allocation. Combined with `par_iter` for large rect counts.

---

## Phase 2: SAT and Density Enforcement Optimizations

**Files:** `src/artwork.rs`, `src/drc.rs`, `src/gdsio.rs`, `src/polygon.rs`

### 2.1 Word-level SAT Construction

**Problem:** `build_sat()` called `bitmap.get(x, y)` per pixel - each call does a bounds check, integer division, and modulo operation.

**Fix:** Added `row_words(y)` method to `ArtworkBitmap` that returns the packed word slice and bit offset for a row. Rewrote `build_sat` to iterate u64 words directly, extracting bits with `(word >> bit) & 1`. No bounds checks, no multiply per pixel.

**Technique:** Walk the word array sequentially, tracking word index and bit position. For each word, extract up to 64 pixel values using shifts. This exploits sequential memory access and eliminates branch mispredictions from bounds checks.

**Measured:**
| Size | Naive | Optimized | Speedup |
|---|---|---|---|
| 512x512 | 187 us | 125 us | **1.50x** |
| 2048x2048 | 3.03 ms | 1.94 ms | **1.56x** |

### 2.2 Popcount-based Window Counting

**Problem:** `count_on_in_window()` counted set pixels in a rectangular region by calling `bitmap.get(x, y)` per pixel - bounds check + division + modulo per call.

**Fix:** Rewrote to operate on raw u64 words using bit masks and hardware `count_ones()` (compiles to `popcnt` on x86). For each row of the window:

1. Compute the bit range `[row_bit_start, row_bit_end)`.
2. If it fits in one word: mask and `count_ones()`.
3. Otherwise: partial first word + full interior words + partial last word, each using `count_ones()`.

**Why 56x:** The naive version does N individual `get()` calls (each: bounds check, divide, modulo, mask, branch). The optimized version processes 64 pixels per `popcnt` instruction and uses no branches in the inner loop. For a 256x256 window (65536 pixels), that is ~1024 `popcnt` ops vs 65536 `get()` calls.

**Measured:**
| Window | Naive | Optimized | Speedup |
|---|---|---|---|
| 256x256 | 29.2 us | 0.52 us | **56x** |

### 2.3 Incremental SAT in enforce_density

**Problem:** `enforce_density()` rebuilds the full SAT from scratch on every iteration (up to 20 iterations), even when only a few pixels changed.

**Fix:** Three changes:

1. **Buffer reuse:** Allocate the SAT buffer once outside the loop. Reuse it across all iterations instead of `vec![0; ...]` each time.

2. **Dirty row tracking:** Track `min_dirty_y` - the lowest y-coordinate of any pixel removed during an iteration.

3. **Partial rebuild:** Added `build_sat_from(bitmap, w, h, sat, start_y)` that only recomputes SAT rows from `start_y` onward. Rows above are unchanged because no pixels were modified there.

**Why it matters:** If density violations cluster in the lower half of the bitmap, subsequent SAT rebuilds skip the entire upper half. In the worst case (violations everywhere), it is equivalent to a full rebuild. In the best case (violations in one window near the bottom), it saves ~95% of SAT work.

**Measured (end-to-end enforce_density):**
| Size | Time | Improvement |
|---|---|---|
| 200x200 | 756 us | **-3.3%** |
| 500x500 | 4.76 ms | **-6.2%** |

The modest end-to-end improvement reflects that SAT construction is only one component. The candidate collection, sorting, and pixel removal loops dominate for small bitmaps. The incremental SAT win grows with bitmap size and iteration count.

### 2.4 DRC Grid Rasterization - Interior/Border Split

**Problem:** DRC density check rasterizes each rectangle's metal area into grid cells. Every overlapping cell computed a clipped intersection using 4 `max()` + 4 `min()` operations, even when the cell was fully inside the rectangle.

**Fix:** For each rectangle, compute the "fully interior" cell range - cells whose boundaries are entirely within the rect. For these cells, accumulate `step * step` directly (one multiply, no clipping). Only border cells (partially covered) use the min/max clipping logic.

**Technique:**
```
Interior x range: ceil((rect.x0 - grid_x0) / step) .. floor((rect.x1 - grid_x0) / step)
Interior y range: ceil((rect.y0 - grid_y0) / step) .. floor((rect.y1 - grid_y0) / step)
```

For large GreedyMerge rectangles spanning many grid cells, most cells are interior. A rect spanning 10x10 cells has 64 interior cells (8x8) and only 36 border cells - the interior path handles 64% of cells with zero clipping math.

### 2.5 Pre-allocated GDS Element Vectors

**Problem:** `write_gds_multi()` and `merge_into_gds_multi()` pushed boundaries one at a time into `cell.elems`, causing repeated `Vec` reallocations for large polygon counts.

**Fix:** Compute `total = sum of all layer rect counts`, then `cell.elems.reserve(total)` before the loop. Single allocation instead of O(log N) reallocations.

### 2.6 Allocation-free Parallel Density Window Iteration

**Problem:** DRC density check built a `Vec<(i32, i32)>` of all window positions before passing to `par_iter`. For large layouts this vector could have millions of entries.

**Fix:** Compute `nx` and `ny` (number of window positions in each axis), then use `(0..nx*ny).into_par_iter()` with index arithmetic to derive `(wx, wy)` on the fly. Zero allocation.

### 2.7 Thread-local Bitset Buffer in Parallel GreedyMerge

**Problem:** Each parallel strip allocated a fresh `used` bitset (`vec![0u64; ...]`). Rayon may process many strips per thread, each allocating and freeing a buffer.

**Fix:** Use `thread_local!` to maintain a reusable `Vec<u64>` per thread. On each strip: `resize()` to needed size, `fill(0)` to clear. The `fill(0)` call compiles to `memset` which is faster than the allocator for buffer reuse.

---

## Benchmark Infrastructure

All optimizations are verified using Criterion benchmarks in `profiling/`:

- `bench_artwork.rs`: SAT construction, window counting, density enforcement. Includes **naive reference implementations** that run alongside optimized code, so every `cargo bench` shows the speedup directly.

- `bench_polygon.rs`: GreedyMerge, RowMerge, PixelRects at various sizes (256 to 4096).

- `bench_drc.rs`: Full DRC and density-only checks at 12K, 50K, and 100K rectangle counts.

Run all benchmarks:
```bash
cargo bench
```

Run a specific benchmark group:
```bash
cargo bench --bench bench_artwork
```

Criterion stores results in `target/criterion/` and automatically reports changes (%) on subsequent runs.
