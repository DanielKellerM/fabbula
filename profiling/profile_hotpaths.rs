//! Profiling binary for samply flame graphs.
//! Usage: cargo build --release --bin profile_hotpaths && samply record ./target/release/profile_hotpaths

use fabbula::artwork::ArtworkBitmap;
use fabbula::drc::{check_density_only, check_drc};
use fabbula::pdk::PdkConfig;
use fabbula::polygon::{PixelPlacement, PolygonStrategy, generate_polygons};

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

fn main() {
    let pdk = PdkConfig::builtin("sky130").unwrap();

    // GreedyMerge at multiple sizes
    for size in [512, 1024, 2048] {
        let bmp = dense_pattern(size);
        eprintln!("GreedyMerge {size}x{size}...");
        for _ in 0..3 {
            let rects = generate_polygons(
                &bmp,
                &pdk,
                &pdk.drc,
                PolygonStrategy::GreedyMerge,
                PixelPlacement::Separated,
            )
            .unwrap();
            std::hint::black_box(&rects);
        }
    }

    // DRC at scale
    eprintln!("DRC 50k rects...");
    let bmp = dense_pattern(512);
    let rects = generate_polygons(
        &bmp,
        &pdk,
        &pdk.drc,
        PolygonStrategy::GreedyMerge,
        PixelPlacement::Separated,
    )
    .unwrap();
    for _ in 0..5 {
        let v = check_drc(&rects, pdk.pdk.db_units_per_um, &pdk.drc);
        std::hint::black_box(&v);
    }

    // Density-only
    let mut pdk_density = pdk.clone();
    pdk_density.drc.density_max = 0.77;
    pdk_density.drc.density_window_um = 50.0;
    eprintln!("Density-only 50k rects...");
    for _ in 0..10 {
        let v = check_density_only(
            &rects,
            pdk_density.pdk.db_units_per_um,
            &pdk_density.drc,
            None,
        );
        std::hint::black_box(&v);
    }

    eprintln!("Done.");
}
