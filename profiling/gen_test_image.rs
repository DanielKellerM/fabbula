//! Generate a large test PNG for benchmarking the full pipeline.
//! Run with: cargo run --bin gen_test_image

use image::{GrayImage, Luma};
use std::path::Path;

fn main() {
    let size = 5120u32;
    let output = Path::new("test_5120.png");

    println!("Generating {size}x{size} checkerboard test image...");
    let mut img = GrayImage::new(size, size);

    for y in 0..size {
        for x in 0..size {
            // 8x8 pixel checkerboard blocks
            let block = ((x / 8) + (y / 8)) % 2;
            let val = if block == 0 { 0u8 } else { 255u8 };
            img.put_pixel(x, y, Luma([val]));
        }
    }

    img.save(output).expect("Failed to save test image");
    println!("Wrote {}", output.display());
}
