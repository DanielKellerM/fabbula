//! Multi-PDK, DRC-aware image-to-GDSII artwork generator.
//!
//! Converts raster images into GDSII layout data for chip top-metal artwork,
//! supporting SKY130, IHP SG13G2, and GF180MCU process design kits.

pub mod artwork;
pub mod color;
pub mod drc;
pub mod gdsio;
pub mod lef;
pub mod pdk;
pub mod polygon;
pub mod preview;
