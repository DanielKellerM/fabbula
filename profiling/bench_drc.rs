use criterion::{Criterion, black_box, criterion_group, criterion_main};
use fabbula::drc::{DrcRule, check_density_only, check_drc};
use fabbula::pdk::PdkConfig;
use fabbula::polygon::Rect;

fn make_grid(side: i32) -> Vec<Rect> {
    let pdk = PdkConfig::builtin("sky130").unwrap();
    let min_w = pdk.um_to_dbu(pdk.drc.min_width);
    let min_s = pdk.um_to_dbu(pdk.drc.min_spacing);
    let size = min_w * 2;
    let pitch = size + min_s;
    let mut rects = Vec::with_capacity((side * side) as usize);
    for iy in 0..side {
        for ix in 0..side {
            rects.push(Rect::new(
                ix * pitch,
                iy * pitch,
                ix * pitch + size,
                iy * pitch + size,
            ));
        }
    }
    rects
}

fn bench_drc_12k_clean(c: &mut Criterion) {
    let pdk = PdkConfig::builtin("sky130").unwrap();
    let rects = make_grid(110);
    c.bench_function("drc_12k_clean", |b| {
        b.iter(|| {
            let v = check_drc(black_box(&rects), pdk.pdk.db_units_per_um, &pdk.drc);
            let structural: Vec<_> = v
                .iter()
                .filter(|v| {
                    matches!(
                        v.rule,
                        DrcRule::MinWidth | DrcRule::MinSpacing | DrcRule::MinArea
                    )
                })
                .collect();
            assert!(structural.is_empty());
        });
    });
}

fn bench_drc_12k_with_density(c: &mut Criterion) {
    let mut pdk = PdkConfig::builtin("sky130").unwrap();
    pdk.drc.density_max = 0.77;
    pdk.drc.density_window_um = 50.0;
    let rects = make_grid(110);
    c.bench_function("drc_12k_with_density", |b| {
        b.iter(|| {
            let _ = check_drc(black_box(&rects), pdk.pdk.db_units_per_um, &pdk.drc);
        });
    });
}

fn bench_drc_50k_clean(c: &mut Criterion) {
    let pdk = PdkConfig::builtin("sky130").unwrap();
    let rects = make_grid(224);
    c.bench_function("drc_50k_clean", |b| {
        b.iter(|| {
            let v = check_drc(black_box(&rects), pdk.pdk.db_units_per_um, &pdk.drc);
            let structural: Vec<_> = v
                .iter()
                .filter(|v| {
                    matches!(
                        v.rule,
                        DrcRule::MinWidth | DrcRule::MinSpacing | DrcRule::MinArea
                    )
                })
                .collect();
            assert!(structural.is_empty());
        });
    });
}

fn bench_drc_100k_clean(c: &mut Criterion) {
    let pdk = PdkConfig::builtin("sky130").unwrap();
    let rects = make_grid(317);
    c.bench_function("drc_100k_clean", |b| {
        b.iter(|| {
            let v = check_drc(black_box(&rects), pdk.pdk.db_units_per_um, &pdk.drc);
            let structural: Vec<_> = v
                .iter()
                .filter(|v| {
                    matches!(
                        v.rule,
                        DrcRule::MinWidth | DrcRule::MinSpacing | DrcRule::MinArea
                    )
                })
                .collect();
            assert!(structural.is_empty());
        });
    });
}

fn bench_density_only_50k(c: &mut Criterion) {
    let mut pdk = PdkConfig::builtin("sky130").unwrap();
    pdk.drc.density_max = 0.77;
    pdk.drc.density_window_um = 50.0;
    let rects = make_grid(224);
    c.bench_function("density_only_50k", |b| {
        b.iter(|| {
            let _ = check_density_only(black_box(&rects), pdk.pdk.db_units_per_um, &pdk.drc, None);
        });
    });
}

criterion_group!(
    benches,
    bench_drc_12k_clean,
    bench_drc_12k_with_density,
    bench_drc_50k_clean,
    bench_drc_100k_clean,
    bench_density_only_50k,
);
criterion_main!(benches);
