use criterion::{Criterion, black_box, criterion_group, criterion_main};
use fabbula::artwork::{ArtworkBitmap, apply_exclusion_mask, enforce_density};
use fabbula::pdk::PdkConfig;
use fabbula::polygon::Rect;

fn solid_bitmap(size: u32) -> ArtworkBitmap {
    ArtworkBitmap::from_bools(size, size, &vec![true; (size * size) as usize])
}

fn dense_bitmap(size: u32) -> ArtworkBitmap {
    let bools: Vec<bool> = (0..size * size)
        .map(|i| {
            let x = i % size;
            let y = i / size;
            !(x + y).is_multiple_of(5)
        })
        .collect();
    ArtworkBitmap::from_bools(size, size, &bools)
}

// --- Naive reference implementations (pre-optimization baseline) ---
// These use per-pixel bitmap.get() with bounds checks, matching the original code.

/// Naive SAT: calls bitmap.get(x, y) per pixel (bounds check + div + mod each time).
fn build_sat_naive(bitmap: &ArtworkBitmap, w: u32, h: u32) -> Vec<u32> {
    let stride = (w + 1) as usize;
    let mut sat = vec![0u32; stride * (h + 1) as usize];
    for y in 0..h {
        let mut row_sum = 0u32;
        for x in 0..w {
            row_sum += bitmap.get(x, y) as u32;
            let idx = (y + 1) as usize * stride + (x + 1) as usize;
            sat[idx] = row_sum + sat[y as usize * stride + (x + 1) as usize];
        }
    }
    sat
}

/// Naive window count: calls bitmap.get(x, y) per pixel.
fn count_on_in_window_naive(bitmap: &ArtworkBitmap, wx: u32, wy: u32, ww: u32, wh: u32) -> u32 {
    let mut count = 0u32;
    for y in wy..wy + wh {
        for x in wx..wx + ww {
            count += bitmap.get(x, y) as u32;
        }
    }
    count
}

// --- Benchmarks ---

fn bench_build_sat(c: &mut Criterion) {
    let bmp = dense_bitmap(512);
    let mut group = c.benchmark_group("build_sat_512");
    group.bench_function("naive", |b| {
        b.iter(|| build_sat_naive(black_box(&bmp), 512, 512));
    });
    group.bench_function("optimized", |b| {
        b.iter(|| fabbula::artwork::build_sat(black_box(&bmp), 512, 512));
    });
    group.finish();
}

fn bench_build_sat_large(c: &mut Criterion) {
    let bmp = dense_bitmap(2048);
    let mut group = c.benchmark_group("build_sat_2048");
    group.bench_function("naive", |b| {
        b.iter(|| build_sat_naive(black_box(&bmp), 2048, 2048));
    });
    group.bench_function("optimized", |b| {
        b.iter(|| fabbula::artwork::build_sat(black_box(&bmp), 2048, 2048));
    });
    group.finish();
}

fn bench_count_window(c: &mut Criterion) {
    let bmp = dense_bitmap(512);
    let mut group = c.benchmark_group("count_window_512");
    group.bench_function("naive", |b| {
        b.iter(|| count_on_in_window_naive(black_box(&bmp), 0, 0, 256, 256));
    });
    group.bench_function("optimized", |b| {
        b.iter(|| fabbula::artwork::count_on_in_window(black_box(&bmp), 0, 0, 256, 256));
    });
    group.finish();
}

fn bench_enforce_density_200(c: &mut Criterion) {
    c.bench_function("enforce_density_200x200_80pct", |b| {
        b.iter_batched(
            || solid_bitmap(200),
            |mut bmp| {
                let _ = enforce_density(black_box(&mut bmp), 0.80, 50);
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_enforce_density_500(c: &mut Criterion) {
    c.bench_function("enforce_density_500x500_80pct", |b| {
        b.iter_batched(
            || solid_bitmap(500),
            |mut bmp| {
                let _ = enforce_density(black_box(&mut bmp), 0.80, 100);
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

/// Create exclusion rects simulating a grid of bond pads over a 512x512 bitmap.
fn make_exclusion_setup(size: u32, num_rects: usize) -> (ArtworkBitmap, Vec<Rect>, PdkConfig) {
    let pdk = PdkConfig::builtin("sky130").unwrap();
    let min_w_um = pdk.snap_to_grid(pdk.drc.min_width);
    let min_s_um = pdk.snap_to_grid(pdk.drc.min_spacing);
    let pitch = pdk.um_to_dbu(min_w_um + min_s_um);
    let pw = pdk.um_to_dbu(min_w_um);

    let bmp = solid_bitmap(size);

    // Create a grid of exclusion rects spread across the bitmap area
    let grid_side = (num_rects as f64).sqrt().ceil() as usize;
    let spacing = (size as i32 * pitch) / grid_side as i32;
    let rect_size = pw * 3; // Each rect covers ~3x3 pixels
    let mut rects = Vec::with_capacity(num_rects);
    for gy in 0..grid_side {
        for gx in 0..grid_side {
            if rects.len() >= num_rects {
                break;
            }
            let cx = gx as i32 * spacing + spacing / 2;
            let cy = gy as i32 * spacing + spacing / 2;
            rects.push(Rect::new(cx, cy, cx + rect_size, cy + rect_size));
        }
    }

    (bmp, rects, pdk)
}

fn bench_exclusion_mask(c: &mut Criterion) {
    let (bmp, rects, pdk) = make_exclusion_setup(512, 64);
    let margin = pdk.um_to_dbu(pdk.snap_to_grid(pdk.drc.min_spacing));

    let mut group = c.benchmark_group("exclusion_mask_512");
    group.bench_function("rasterized", |b| {
        b.iter_batched(
            || bmp.clone(),
            |mut bmp| {
                apply_exclusion_mask(
                    black_box(&mut bmp),
                    black_box(&rects),
                    black_box(&pdk),
                    black_box(margin),
                );
            },
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_exclusion_mask_large(c: &mut Criterion) {
    let (bmp, rects, pdk) = make_exclusion_setup(2048, 256);
    let margin = pdk.um_to_dbu(pdk.snap_to_grid(pdk.drc.min_spacing));

    let mut group = c.benchmark_group("exclusion_mask_2048");
    group.bench_function("rasterized", |b| {
        b.iter_batched(
            || bmp.clone(),
            |mut bmp| {
                apply_exclusion_mask(
                    black_box(&mut bmp),
                    black_box(&rects),
                    black_box(&pdk),
                    black_box(margin),
                );
            },
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_build_sat,
    bench_build_sat_large,
    bench_count_window,
    bench_enforce_density_200,
    bench_enforce_density_500,
    bench_exclusion_mask,
    bench_exclusion_mask_large,
);
criterion_main!(benches);
