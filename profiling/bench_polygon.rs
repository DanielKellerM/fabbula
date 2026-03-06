use criterion::{black_box, criterion_group, criterion_main, Criterion};
use fabbula::artwork::ArtworkBitmap;
use fabbula::pdk::PdkConfig;
use fabbula::polygon::{generate_polygons, PolygonStrategy};

/// ~80% density pattern - matches typical PDK density targets.
/// Pixel is off when (x + y) % 5 == 0, giving 80% metal fill.
fn dense_pattern(size: u32) -> ArtworkBitmap {
    let bools: Vec<bool> = (0..size * size)
        .map(|i| {
            let x = i % size;
            let y = i / size;
            (x + y) % 5 != 0
        })
        .collect();
    ArtworkBitmap::from_bools(size, size, &bools)
}

fn bench_greedy_merge_256(c: &mut Criterion) {
    let bmp = dense_pattern(256);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    c.bench_function("greedy_merge_256", |b| {
        b.iter(|| {
            let _ = generate_polygons(black_box(&bmp), &pdk, PolygonStrategy::GreedyMerge, false);
        });
    });
}

fn bench_greedy_merge_512(c: &mut Criterion) {
    let bmp = dense_pattern(512);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    c.bench_function("greedy_merge_512", |b| {
        b.iter(|| {
            let _ = generate_polygons(black_box(&bmp), &pdk, PolygonStrategy::GreedyMerge, false);
        });
    });
}

fn bench_row_merge_512(c: &mut Criterion) {
    let bmp = dense_pattern(512);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    c.bench_function("row_merge_512", |b| {
        b.iter(|| {
            let _ = generate_polygons(black_box(&bmp), &pdk, PolygonStrategy::RowMerge, false);
        });
    });
}

fn bench_pixel_rects_512(c: &mut Criterion) {
    let bmp = dense_pattern(512);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    c.bench_function("pixel_rects_512", |b| {
        b.iter(|| {
            let _ = generate_polygons(black_box(&bmp), &pdk, PolygonStrategy::PixelRects, false);
        });
    });
}

fn bench_greedy_merge_2048(c: &mut Criterion) {
    let bmp = dense_pattern(2048);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    c.bench_function("greedy_merge_2048", |b| {
        b.iter(|| {
            let _ = generate_polygons(black_box(&bmp), &pdk, PolygonStrategy::GreedyMerge, false);
        });
    });
}

fn bench_row_merge_2048(c: &mut Criterion) {
    let bmp = dense_pattern(2048);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    c.bench_function("row_merge_2048", |b| {
        b.iter(|| {
            let _ = generate_polygons(black_box(&bmp), &pdk, PolygonStrategy::RowMerge, false);
        });
    });
}

fn bench_greedy_merge_4096(c: &mut Criterion) {
    let bmp = dense_pattern(4096);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    c.bench_function("greedy_merge_4096", |b| {
        b.iter(|| {
            let _ = generate_polygons(black_box(&bmp), &pdk, PolygonStrategy::GreedyMerge, false);
        });
    });
}

criterion_group!(
    benches,
    bench_greedy_merge_256,
    bench_greedy_merge_512,
    bench_row_merge_512,
    bench_pixel_rects_512,
    bench_greedy_merge_2048,
    bench_row_merge_2048,
    bench_greedy_merge_4096,
);
criterion_main!(benches);
