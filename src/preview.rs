// Copyright 2026 Daniel Keller <daniel.keller.m@gmail.com>
// Licensed under the Apache License, Version 2.0.
// SPDX-License-Identifier: Apache-2.0

//! SVG and interactive HTML preview generation.
//!
//! Renders artwork polygons as static SVG images or self-contained HTML files
//! with pan, zoom, and hover-to-inspect functionality.

use crate::pdk::PdkConfig;
use crate::polygon::Rect;
use anyhow::{Context, Result};
use std::io::{BufWriter, Write};
use std::path::Path;

/// Default layer colors for multi-layer SVG/HTML preview
pub const DEFAULT_LAYER_COLORS: &[&str] = &[
    "#c0c0c0", "#e8a87c", "#85cdca", "#d5a6bd", "#a8d8ea", "#f6c85f",
];

/// Write a multi-layer SVG preview.
///
/// Each entry in `layers` is `(rects, fill_color)`.
pub fn write_svg_multi(
    layers: &[(&[Rect], &str)],
    output: &Path,
    scale: f64,
    background: Option<&str>,
) -> Result<()> {
    let all_rects: Vec<&Rect> = layers.iter().flat_map(|(r, _)| r.iter()).collect();
    let bb = if all_rects.is_empty() {
        Rect::new(0, 0, 1000, 1000)
    } else {
        let first = *all_rects[0];
        all_rects[1..].iter().fold(first, |bb, r| Rect {
            x0: bb.x0.min(r.x0),
            y0: bb.y0.min(r.y0),
            x1: bb.x1.max(r.x1),
            y1: bb.y1.max(r.y1),
        })
    };

    let margin = ((bb.width().max(bb.height())) as f64 * 0.02) as i32;
    let vb_x = bb.x0 - margin;
    let vb_y = bb.y0 - margin;
    let vb_w = bb.width() + 2 * margin;
    let vb_h = bb.height() + 2 * margin;

    let svg_w = (vb_w as f64 * scale) as u32;
    let svg_h = (vb_h as f64 * scale) as u32;

    let file = std::fs::File::create(output)
        .with_context(|| format!("Failed to create SVG: {}", output.display()))?;
    let mut f = BufWriter::new(file);

    writeln!(
        f,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg"
     width="{}" height="{}"
     viewBox="{} {} {} {}">"#,
        svg_w, svg_h, vb_x, vb_y, vb_w, vb_h
    )?;

    if let Some(bg) = background {
        writeln!(
            f,
            r#"  <rect x="{}" y="{}" width="{}" height="{}" fill="{}"/>"#,
            vb_x, vb_y, vb_w, vb_h, bg
        )?;
    }

    writeln!(
        f,
        r#"  <g transform="translate(0, {}) scale(1, -1)">"#,
        vb_y * 2 + vb_h
    )?;

    for (rects, fill_color) in layers {
        for rect in *rects {
            writeln!(
                f,
                r#"    <rect x="{}" y="{}" width="{}" height="{}" fill="{}"/>"#,
                rect.x0,
                rect.y0,
                rect.width(),
                rect.height(),
                fill_color
            )?;
        }
    }

    writeln!(f, "  </g>")?;
    writeln!(f, "</svg>")?;

    tracing::info!(
        "Wrote SVG preview: {} ({}x{}, {} layers)",
        output.display(),
        svg_w,
        svg_h,
        layers.len()
    );
    Ok(())
}

/// Write an SVG preview of the generated polygons (single layer).
pub fn write_svg(
    rects: &[Rect],
    output: &Path,
    scale: f64,
    fill_color: &str,
    background: Option<&str>,
) -> Result<()> {
    write_svg_multi(&[(rects, fill_color)], output, scale, background)
}

/// Multi-layer entry for HTML preview
pub struct HtmlLayer<'a> {
    pub rects: &'a [Rect],
    pub name: &'a str,
    pub color: &'a str,
}

/// Write a multi-layer interactive HTML preview.
pub fn write_html_preview_multi(
    layers: &[HtmlLayer],
    output: &Path,
    pdk: &PdkConfig,
) -> Result<()> {
    let all_rects: Vec<&Rect> = layers.iter().flat_map(|l| l.rects.iter()).collect();
    let bb = if all_rects.is_empty() {
        Rect::new(0, 0, 1000, 1000)
    } else {
        let first = *all_rects[0];
        all_rects[1..].iter().fold(first, |bb, r| Rect {
            x0: bb.x0.min(r.x0),
            y0: bb.y0.min(r.y0),
            x1: bb.x1.max(r.x1),
            y1: bb.y1.max(r.y1),
        })
    };
    let total_polys: usize = layers.iter().map(|l| l.rects.len()).sum();
    write_html_preview_inner(&all_rects, layers, &bb, total_polys, output, pdk)
}

/// Write a self-contained interactive HTML preview (single layer).
pub fn write_html_preview(rects: &[Rect], output: &Path, pdk: &PdkConfig) -> Result<()> {
    let single = [HtmlLayer {
        rects,
        name: &pdk.artwork_layer.name,
        color: "#b0c4de",
    }];
    write_html_preview_multi(&single, output, pdk)
}

fn write_html_preview_inner(
    _all_rects: &[&Rect],
    layers: &[HtmlLayer],
    bb: &Rect,
    total_polys: usize,
    output: &Path,
    pdk: &PdkConfig,
) -> Result<()> {
    let bb = *bb;
    let dbu = pdk.pdk.db_units_per_um as f64;
    let width_um = bb.width() as f64 / dbu;
    let height_um = bb.height() as f64 / dbu;

    let margin = ((bb.width().max(bb.height())) as f64 * 0.02) as i32;
    let vb_x = bb.x0 - margin;
    let vb_y = bb.y0 - margin;
    let vb_w = bb.width() + 2 * margin;
    let vb_h = bb.height() + 2 * margin;
    let flip_y = vb_y * 2 + vb_h;
    let sw = (vb_w.max(vb_h) as f64 * 0.002) as i32;

    let file = std::fs::File::create(output)
        .with_context(|| format!("Failed to create HTML preview: {}", output.display()))?;
    let mut f = BufWriter::new(file);

    // Build layer legend HTML
    let mut legend_html = String::new();
    if layers.len() > 1 {
        legend_html.push_str(
            "<div style=\"margin-top:8px;border-top:1px solid #30363d;padding-top:6px\">",
        );
        legend_html.push_str("<div style=\"color:#8b949e;margin-bottom:4px\">Layers:</div>");
        for layer in layers {
            legend_html.push_str(&format!(
                "<div><span style=\"display:inline-block;width:12px;height:12px;background:{};border-radius:2px;vertical-align:middle;margin-right:6px\"></span>{} ({} polys)</div>",
                layer.color, layer.name, layer.rects.len()
            ));
        }
        legend_html.push_str("</div>");
    }

    // Part 1: Header
    write!(
        f,
        r##"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>fabbula - {pdk_name} artwork preview</title>
<style>
  * {{ margin:0; padding:0; box-sizing:border-box; }}
  body {{ background:#0d1117; overflow:hidden; font-family:monospace; color:#c9d1d9; }}
  #info {{ position:fixed; top:10px; left:10px; background:rgba(13,17,23,0.9); padding:10px 14px;
           border:1px solid #30363d; border-radius:6px; font-size:13px; z-index:10; }}
  #info h3 {{ margin-bottom:6px; color:#58a6ff; font-size:14px; }}
  #info div {{ margin:2px 0; }}
  #tooltip {{ position:fixed; display:none; background:rgba(13,17,23,0.95); padding:6px 10px;
              border:1px solid #58a6ff; border-radius:4px; font-size:12px; pointer-events:none; z-index:20; }}
  svg {{ display:block; }}
  .r {{ opacity:0.85; }}
  .r:hover {{ opacity:1; stroke:#58a6ff; stroke-width:{sw}; }}
</style></head><body>
<div id="info">
  <h3>fabbula preview</h3>
  <div>PDK: {pdk_name}</div>
  <div>Polygons: {poly_count}</div>
  <div>Size: {width_um:.1} x {height_um:.1} um</div>
  {legend}
  <div style="margin-top:6px;color:#8b949e">Scroll to zoom, drag to pan</div>
</div>
<div id="tooltip"></div>
<svg id="canvas" xmlns="http://www.w3.org/2000/svg" width="100%" height="100%"
     viewBox="{vb_x} {vb_y} {vb_w} {vb_h}">
  <rect x="{vb_x}" y="{vb_y}" width="{vb_w}" height="{vb_h}" fill="#0d1117"/>
  <g id="art" transform="translate(0, {flip_y}) scale(1, -1)">
"##,
        pdk_name = pdk.pdk.name,
        poly_count = total_polys,
        width_um = width_um,
        height_um = height_um,
        legend = legend_html,
        vb_x = vb_x,
        vb_y = vb_y,
        vb_w = vb_w,
        vb_h = vb_h,
        flip_y = flip_y,
        sw = sw,
    )?;

    // Part 2: Stream rects directly, per layer
    for layer in layers {
        for rect in layer.rects {
            writeln!(
                f,
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\" class=\"r\" \
                 data-x0=\"{}\" data-y0=\"{}\" data-x1=\"{}\" data-y1=\"{}\" data-layer=\"{}\"/>",
                rect.x0,
                rect.y0,
                rect.width(),
                rect.height(),
                layer.color,
                rect.x0,
                rect.y0,
                rect.x1,
                rect.y1,
                layer.name
            )?;
        }
    }

    // Part 3: Footer
    write!(
        f,
        r##"  </g>
</svg>
<script>
(function(){{
  const svg=document.getElementById('canvas');
  const tooltip=document.getElementById('tooltip');
  const dbu={dbu};
  let vb={{x:{vb_x},y:{vb_y},w:{vb_w},h:{vb_h}}};
  function setVB(){{ svg.setAttribute('viewBox', vb.x+' '+vb.y+' '+vb.w+' '+vb.h); }}

  // Zoom
  svg.addEventListener('wheel', function(e){{
    e.preventDefault();
    const s=e.deltaY>0?1.15:1/1.15;
    const pt=svg.createSVGPoint();
    pt.x=e.clientX; pt.y=e.clientY;
    const svgP=pt.matrixTransform(svg.getScreenCTM().inverse());
    vb.x=svgP.x-(svgP.x-vb.x)*s;
    vb.y=svgP.y-(svgP.y-vb.y)*s;
    vb.w*=s; vb.h*=s;
    setVB();
  }});

  // Pan
  let dragging=false, lastX, lastY;
  svg.addEventListener('mousedown', function(e){{ dragging=true; lastX=e.clientX; lastY=e.clientY; svg.style.cursor='grabbing'; }});
  window.addEventListener('mousemove', function(e){{
    if(!dragging) return;
    const dx=(e.clientX-lastX)*vb.w/svg.clientWidth;
    const dy=(e.clientY-lastY)*vb.h/svg.clientHeight;
    vb.x-=dx; vb.y-=dy;
    lastX=e.clientX; lastY=e.clientY;
    setVB();
  }});
  window.addEventListener('mouseup', function(){{ dragging=false; svg.style.cursor='default'; }});

  // Tooltip
  svg.addEventListener('mouseover', function(e){{
    const t=e.target;
    if(!t.classList.contains('r')) return;
    const x0=(+t.dataset.x0/dbu).toFixed(3);
    const y0=(+t.dataset.y0/dbu).toFixed(3);
    const x1=(+t.dataset.x1/dbu).toFixed(3);
    const y1=(+t.dataset.y1/dbu).toFixed(3);
    const w=((t.dataset.x1-t.dataset.x0)/dbu).toFixed(3);
    const h=((t.dataset.y1-t.dataset.y0)/dbu).toFixed(3);
    const layer=t.dataset.layer||'';
    tooltip.innerHTML=(layer?layer+': ':'')+'('+x0+', '+y0+') - ('+x1+', '+y1+') um<br>'+w+' x '+h+' um';
    tooltip.style.display='block';
  }});
  svg.addEventListener('mousemove', function(e){{
    if(tooltip.style.display==='block'){{ tooltip.style.left=(e.clientX+12)+'px'; tooltip.style.top=(e.clientY+12)+'px'; }}
  }});
  svg.addEventListener('mouseout', function(e){{
    if(e.target.classList.contains('r')) tooltip.style.display='none';
  }});
}})();
</script></body></html>
"##,
        dbu = dbu,
        vb_x = vb_x,
        vb_y = vb_y,
        vb_w = vb_w,
        vb_h = vb_h,
    )?;

    tracing::info!(
        "Wrote HTML preview: {} ({} polygons, {} layers)",
        output.display(),
        total_polys,
        layers.len()
    );
    Ok(())
}
