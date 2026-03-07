use criterion::{Criterion, black_box, criterion_group, criterion_main};
use fabbula::artwork::ArtworkBitmap;
use fabbula::drc::check_drc;
use fabbula::pdk::PdkConfig;
use fabbula::polygon::{PolygonStrategy, generate_polygons};
use std::time::Duration;

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

fn bench_greedy_merge_6250(c: &mut Criterion) {
    let bmp = dense_pattern(6250);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    let mut group = c.benchmark_group("die_scale");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(30));
    group.bench_function("greedy_merge_6250", |b| {
        b.iter(|| {
            let _ = generate_polygons(
                black_box(&bmp),
                &pdk,
                &pdk.drc,
                PolygonStrategy::GreedyMerge,
                false,
            );
        });
    });
    group.finish();
}

fn bench_greedy_merge_15625(c: &mut Criterion) {
    let bmp = dense_pattern(15625);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    let mut group = c.benchmark_group("die_scale");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(60));
    group.bench_function("greedy_merge_15625", |b| {
        b.iter(|| {
            let _ = generate_polygons(
                black_box(&bmp),
                &pdk,
                &pdk.drc,
                PolygonStrategy::GreedyMerge,
                false,
            );
        });
    });
    group.finish();
}

fn bench_drc_6250(c: &mut Criterion) {
    let bmp = dense_pattern(6250);
    let pdk = PdkConfig::builtin("sky130").unwrap();
    let rects =
        generate_polygons(&bmp, &pdk, &pdk.drc, PolygonStrategy::GreedyMerge, false).unwrap();
    let mut group = c.benchmark_group("die_scale");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(30));
    group.bench_function("drc_6250", |b| {
        b.iter(|| {
            let _ = check_drc(black_box(&rects), pdk.pdk.db_units_per_um, &pdk.drc);
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_greedy_merge_6250,
    bench_greedy_merge_15625,
    bench_drc_6250,
);
criterion_main!(benches);
