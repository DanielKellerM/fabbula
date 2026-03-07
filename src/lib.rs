// Copyright 2026 Daniel Keller <daniel.keller.m@gmail.com>
// Licensed under the Apache License, Version 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Multi-PDK, DRC-aware image-to-GDSII artwork generator.
//!
//! fabbula converts raster images into DRC-clean GDSII layout data suitable for
//! chip top-metal artwork. It supports multiple process design kits including
//! SKY130, IHP SG13G2, GF180MCU, FreePDK45, ASAP7, and a built-in demo PDK.
//!
//! # Pipeline
//!
//! The conversion follows a four-stage pipeline:
//!
//! 1. **Image loading** - load and threshold a raster image into a binary bitmap
//! 2. **Bitmap processing** - apply exclusion masks and density enforcement
//! 3. **Polygon generation** - convert bitmap pixels to rectangles in database units
//! 4. **GDSII output** - write rectangles as boundary elements to a GDS file
//!
//! DRC compliance is guaranteed by construction: each bitmap pixel maps to a
//! `min_width x min_width` rectangle, spaced at `min_width + min_spacing` pitch.
//!
//! # Example
//!
//! ```no_run
//! use fabbula::pdk::PdkConfig;
//! use fabbula::artwork::{load_artwork, ThresholdMode, DitherMode};
//! use fabbula::polygon::{generate_polygons, PolygonStrategy, PixelPlacement};
//! use fabbula::drc::{check_drc, report_drc};
//! use fabbula::gdsio::write_gds;
//! use std::path::Path;
//!
//! let pdk = PdkConfig::builtin("sky130").unwrap();
//! let bitmap = load_artwork(Path::new("logo.png"), ThresholdMode::Otsu, None, DitherMode::Off).unwrap();
//! let rects = generate_polygons(&bitmap, &pdk, &pdk.drc, PolygonStrategy::GreedyMerge, PixelPlacement::Separated).unwrap();
//! let violations = check_drc(&rects, pdk.pdk.db_units_per_um, &pdk.drc);
//! report_drc(&violations);
//! write_gds(&rects, &pdk, "artwork", Path::new("output.gds")).unwrap();
//! ```
//!
//! # Modules
//!
//! - [`artwork`] - Image loading, thresholding, and density enforcement
//! - [`color`] - Multi-layer color extraction (channel splitting, palette quantization)
//! - [`drc`] - Design rule checking with R-tree spatial indexing
//! - [`gdsio`] - GDSII file reading, writing, and merging
//! - [`lef`] - LEF macro output for place-and-route tools
//! - [`pdk`] - PDK configuration loading and built-in PDK support
//! - [`polygon`] - Bitmap-to-rectangle conversion strategies
//! - [`preview`] - SVG and interactive HTML preview generation
//! - [`tiles`] - Tile pyramid generation for deep zoom previews

pub mod artwork;
pub mod color;
pub mod drc;
pub mod gdsio;
pub mod generation;
pub mod lef;
pub mod pdk;
pub mod polygon;
pub mod preview;
pub mod tiles;

// Re-export key types for convenient library usage
pub use artwork::{ArtworkBitmap, DitherMode, ThresholdMode};
pub use drc::{DrcRule, DrcViolation};
pub use pdk::{BuiltinPdk, DrcRules, LayerVariant, PdkConfig};
pub use polygon::{PixelPlacement, Point, PolygonStrategy, Rect};
