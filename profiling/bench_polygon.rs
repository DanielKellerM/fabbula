use criterion::{Criterion, black_box, criterion_group, criterion_main};
use fabbula::artwork::ArtworkBitmap;
use fabbula::pdk::PdkConfig;
use fabbula::polygon::{PixelPlacement, PolygonStrategy, generate_polygons};

/// ~80% density pattern - matches typical PDK density targets.
/// Pixel is off when (x + y) % 5 == 0, giving 80% metal fill.
fn dense_pattern(size: u32) -> ArtworkBitmap {
    let bools: Vec<bool> = (0..size * size)
        .map(|i| {
            let x = i % size;
            let y = i / size;
            !(x + y).is_multiple_of(5)
        })
        .collect();
    ArtworkBitmap::from_bools(size, size, &bools)
}

fn bench_greedy_merge_256(c: &mut Criterion) {
    let bmp = dense_pattern(256);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    c.bench_function("greedy_merge_256", |b| {
        b.iter(|| {
            let _ = generate_polygons(
                black_box(&bmp),
                &pdk,
                &pdk.drc,
                PolygonStrategy::GreedyMerge,
                PixelPlacement::Separated,
            );
        });
    });
}

fn bench_greedy_merge_512(c: &mut Criterion) {
    let bmp = dense_pattern(512);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    c.bench_function("greedy_merge_512", |b| {
        b.iter(|| {
            let _ = generate_polygons(
                black_box(&bmp),
                &pdk,
                &pdk.drc,
                PolygonStrategy::GreedyMerge,
                PixelPlacement::Separated,
            );
        });
    });
}

fn bench_row_merge_512(c: &mut Criterion) {
    let bmp = dense_pattern(512);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    c.bench_function("row_merge_512", |b| {
        b.iter(|| {
            let _ = generate_polygons(
                black_box(&bmp),
                &pdk,
                &pdk.drc,
                PolygonStrategy::RowMerge,
                PixelPlacement::Separated,
            );
        });
    });
}

fn bench_pixel_rects_512(c: &mut Criterion) {
    let bmp = dense_pattern(512);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    c.bench_function("pixel_rects_512", |b| {
        b.iter(|| {
            let _ = generate_polygons(
                black_box(&bmp),
                &pdk,
                &pdk.drc,
                PolygonStrategy::PixelRects,
                PixelPlacement::Separated,
            );
        });
    });
}

fn bench_greedy_merge_2048(c: &mut Criterion) {
    let bmp = dense_pattern(2048);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    c.bench_function("greedy_merge_2048", |b| {
        b.iter(|| {
            let _ = generate_polygons(
                black_box(&bmp),
                &pdk,
                &pdk.drc,
                PolygonStrategy::GreedyMerge,
                PixelPlacement::Separated,
            );
        });
    });
}

fn bench_row_merge_2048(c: &mut Criterion) {
    let bmp = dense_pattern(2048);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    c.bench_function("row_merge_2048", |b| {
        b.iter(|| {
            let _ = generate_polygons(
                black_box(&bmp),
                &pdk,
                &pdk.drc,
                PolygonStrategy::RowMerge,
                PixelPlacement::Separated,
            );
        });
    });
}

fn bench_greedy_merge_4096(c: &mut Criterion) {
    let bmp = dense_pattern(4096);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    c.bench_function("greedy_merge_4096", |b| {
        b.iter(|| {
            let _ = generate_polygons(
                black_box(&bmp),
                &pdk,
                &pdk.drc,
                PolygonStrategy::GreedyMerge,
                PixelPlacement::Separated,
            );
        });
    });
}

fn bench_histogram_merge_256(c: &mut Criterion) {
    let bmp = dense_pattern(256);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    c.bench_function("histogram_merge_256", |b| {
        b.iter(|| {
            let _ = generate_polygons(
                black_box(&bmp),
                &pdk,
                &pdk.drc,
                PolygonStrategy::HistogramMerge,
                PixelPlacement::Separated,
            );
        });
    });
}

fn bench_histogram_merge_512(c: &mut Criterion) {
    let bmp = dense_pattern(512);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    c.bench_function("histogram_merge_512", |b| {
        b.iter(|| {
            let _ = generate_polygons(
                black_box(&bmp),
                &pdk,
                &pdk.drc,
                PolygonStrategy::HistogramMerge,
                PixelPlacement::Separated,
            );
        });
    });
}

fn bench_histogram_merge_2048(c: &mut Criterion) {
    let bmp = dense_pattern(2048);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    c.bench_function("histogram_merge_2048", |b| {
        b.iter(|| {
            let _ = generate_polygons(
                black_box(&bmp),
                &pdk,
                &pdk.drc,
                PolygonStrategy::HistogramMerge,
                PixelPlacement::Separated,
            );
        });
    });
}

fn bench_histogram_merge_4096(c: &mut Criterion) {
    let bmp = dense_pattern(4096);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    c.bench_function("histogram_merge_4096", |b| {
        b.iter(|| {
            let _ = generate_polygons(
                black_box(&bmp),
                &pdk,
                &pdk.drc,
                PolygonStrategy::HistogramMerge,
                PixelPlacement::Separated,
            );
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
    bench_histogram_merge_256,
    bench_histogram_merge_512,
    bench_histogram_merge_2048,
    bench_histogram_merge_4096,
);
criterion_main!(benches);
