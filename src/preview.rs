// Copyright 2026 Daniel Keller <daniel.keller.m@gmail.com>
// Licensed under the Apache License, Version 2.0.
// SPDX-License-Identifier: Apache-2.0

//! SVG and interactive HTML preview generation.
//!
//! Renders artwork polygons as static SVG images or self-contained HTML files
//! with pan, zoom, and hover-to-inspect functionality.

use crate::pdk::PdkConfig;
use crate::polygon::{Rect, bounding_box_refs};
use anyhow::{Context, Result};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

/// Default layer colors for multi-layer SVG/HTML preview
pub const DEFAULT_LAYER_COLORS: &[&str] = &[
    "#c0c0c0", "#e8a87c", "#85cdca", "#d5a6bd", "#a8d8ea", "#f6c85f",
];

/// Multi-layer entry for SVG preview
pub struct SvgLayer<'a> {
    pub rects: &'a [Rect],
    pub color: &'a str,
}

/// Write a multi-layer SVG preview.
pub fn write_svg_multi(
    layers: &[SvgLayer],
    output: &Path,
    scale: f64,
    background: Option<&str>,
) -> Result<()> {
    let all_rects: Vec<&Rect> = layers.iter().flat_map(|l| l.rects.iter()).collect();
    if all_rects.len() > 50_000 {
        tracing::warn!(
            "SVG output contains {} rectangles and may be very large. \
             Consider using --html with --deep-zoom for large designs.",
            all_rects.len()
        );
    }
    let bb = bounding_box_refs(&all_rects).unwrap_or(Rect::new(0, 0, 1000, 1000));

    let margin = (bb.width().max(bb.height()).0 as f64 * 0.02) as i32;
    let vb_x = bb.x0.0 - margin;
    let vb_y = bb.y0.0 - margin;
    let vb_w = bb.width().0 + 2 * margin;
    let vb_h = bb.height().0 + 2 * margin;

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

    for layer in layers {
        for rect in layer.rects {
            writeln!(
                f,
                r#"    <rect x="{}" y="{}" width="{}" height="{}" fill="{}"/>"#,
                rect.x0.0,
                rect.y0.0,
                rect.width().0,
                rect.height().0,
                layer.color
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
    write_svg_multi(
        &[SvgLayer {
            rects,
            color: fill_color,
        }],
        output,
        scale,
        background,
    )
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
    let bb = bounding_box_refs(&all_rects).unwrap_or(Rect::new(0, 0, 1000, 1000));
    let total_polys: usize = layers.iter().map(|l| l.rects.len()).sum();
    write_html_preview_inner(layers, &bb, total_polys, output, pdk)
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
    layers: &[HtmlLayer],
    bb: &Rect,
    total_polys: usize,
    output: &Path,
    pdk: &PdkConfig,
) -> Result<()> {
    let bb = *bb;
    let dbu = pdk.pdk.db_units_per_um as f64;
    let width_um = bb.width().0 as f64 / dbu;
    let height_um = bb.height().0 as f64 / dbu;

    let margin = (bb.width().max(bb.height()).0 as f64 * 0.02) as i32;
    let vb_x = bb.x0.0 - margin;
    let vb_y = bb.y0.0 - margin;
    let vb_w = bb.width().0 + 2 * margin;
    let vb_h = bb.height().0 + 2 * margin;
    let flip_y = vb_y * 2 + vb_h;
    let sw = (vb_w.max(vb_h) as f64 * 0.002) as i32;

    let file = std::fs::File::create(output)
        .with_context(|| format!("Failed to create HTML preview: {}", output.display()))?;
    let mut f = BufWriter::new(file);

    // Build layer legend HTML (starts with newline+indent so it fits after the previous div)
    let mut legend_html = String::new();
    if layers.len() > 1 {
        legend_html.push_str(
            "\n  <div style=\"margin-top:8px;border-top:1px solid #30363d;padding-top:6px\">",
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
  :root {{ --bg:#0d1117; --fg:#c9d1d9; --panel:rgba(13,17,23,0.9); --border:#30363d; --accent:#58a6ff; --dim:#8b949e; }}
  [data-theme="light"] {{ --bg:#f5f5f0; --fg:#3a3530; --panel:rgba(255,255,255,0.92); --border:#d0ccc4; --accent:#3a6e96; --dim:#8a8478; }}
  body {{ background:var(--bg); overflow:hidden; font-family:monospace; color:var(--fg); transition:background 0.3s,color 0.3s; }}
  #info {{ position:fixed; top:10px; left:10px; background:var(--panel); padding:10px 14px;
           border:1px solid var(--border); border-radius:6px; font-size:13px; z-index:10; transition:background 0.3s,border-color 0.3s; }}
  #info h3 {{ margin-bottom:6px; color:var(--accent); font-size:14px; }}
  #info div {{ margin:2px 0; }}
  #themeBtn {{ position:fixed; top:10px; right:10px; background:var(--panel); border:1px solid var(--border);
               border-radius:6px; padding:6px 12px; cursor:pointer; font-family:monospace; font-size:13px;
               color:var(--dim); z-index:10; transition:background 0.3s,border-color 0.3s,color 0.3s; }}
  #themeBtn:hover {{ color:var(--accent); border-color:var(--accent); }}
  #tooltip {{ position:fixed; display:none; background:var(--panel); padding:6px 10px;
              border:1px solid var(--accent); border-radius:4px; font-size:12px; pointer-events:none; z-index:20; }}
  svg {{ display:block; }}
  .r {{ opacity:0.85; }}
  .r:hover {{ opacity:1; stroke:var(--accent); stroke-width:{sw}; }}
  #bgRect {{ transition:fill 0.3s; }}
</style>
<script>
(function(){{
  var t=localStorage.getItem('fabbula-theme');
  if(t) document.documentElement.setAttribute('data-theme',t);
}})();
</script></head><body>
<button id="themeBtn" onclick="(function(){{var r=document.documentElement,c=r.getAttribute('data-theme')==='light'?'dark':'light';r.setAttribute('data-theme',c);localStorage.setItem('fabbula-theme',c);document.getElementById('bgRect').setAttribute('fill',c==='light'?'#f5f5f0':'#0d1117');document.getElementById('themeBtn').textContent=c==='light'?'light':'dark';}})()">dark</button>
<div id="info">
  <h3>fabbula preview</h3>
  <div>PDK: {pdk_name}</div>
  <div>Polygons: {poly_count}</div>
  <div>Size: {width_um:.1} x {height_um:.1} um</div>{legend}
  <div style="margin-top:6px;color:var(--dim)">Scroll to zoom, drag to pan</div>
</div>
<div id="tooltip"></div>
<svg id="canvas" xmlns="http://www.w3.org/2000/svg" width="100%" height="100%"
     viewBox="{vb_x} {vb_y} {vb_w} {vb_h}">
  <rect id="bgRect" x="{vb_x}" y="{vb_y}" width="{vb_w}" height="{vb_h}" fill="#0d1117"/>
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
                rect.x0.0,
                rect.y0.0,
                rect.width().0,
                rect.height().0,
                layer.color,
                rect.x0.0,
                rect.y0.0,
                rect.x1.0,
                rect.y1.0,
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

  // Apply saved theme on load
  var th=localStorage.getItem('fabbula-theme')||'dark';
  document.getElementById('themeBtn').textContent=th;
  if(th==='light') document.getElementById('bgRect').setAttribute('fill','#f5f5f0');
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

/// Deep zoom HTML preview configuration.
pub struct DeepZoomConfig {
    /// Path to the tile directory (relative to the HTML file).
    pub tile_dir_rel: String,
    /// Total polygon count across all layers.
    pub total_polys: usize,
    /// Artwork width in micrometers.
    pub width_um: f64,
    /// Artwork height in micrometers.
    pub height_um: f64,
}

/// Write a deep zoom HTML preview with tile-based rendering.
///
/// The HTML uses a canvas element for rendering PNG tiles at overview zoom levels,
/// transitioning to SVG polygon overlay when zoomed in close enough for hover/inspect.
pub fn write_deep_zoom_preview(
    layers: &[HtmlLayer],
    output: &Path,
    pdk: &PdkConfig,
    tile_dir: &Path,
) -> Result<PathBuf> {
    let all_rects: Vec<&Rect> = layers.iter().flat_map(|l| l.rects.iter()).collect();
    let bb = bounding_box_refs(&all_rects).unwrap_or(Rect::new(0, 0, 1000, 1000));
    let total_polys: usize = layers.iter().map(|l| l.rects.len()).sum();
    let dbu = pdk.pdk.db_units_per_um as f64;
    let width_um = bb.width().0 as f64 / dbu;
    let height_um = bb.height().0 as f64 / dbu;

    // Compute tile dir relative path from HTML file location
    let html_dir = output.parent().unwrap_or(Path::new("."));
    let tile_dir_rel = tile_dir
        .strip_prefix(html_dir)
        .unwrap_or(tile_dir)
        .to_string_lossy()
        .to_string();

    // Build layer legend HTML
    let mut legend_html = String::new();
    if layers.len() > 1 {
        legend_html.push_str(
            "\n  <div style=\"margin-top:8px;border-top:1px solid #30363d;padding-top:6px\">",
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

    let file = std::fs::File::create(output)
        .with_context(|| format!("Failed to create HTML preview: {}", output.display()))?;
    let mut f = BufWriter::new(file);

    write!(
        f,
        r##"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>fabbula - {pdk_name} deep zoom preview</title>
<style>
  * {{ margin:0; padding:0; box-sizing:border-box; }}
  :root {{ --bg:#0d1117; --fg:#c9d1d9; --panel:rgba(13,17,23,0.9); --border:#30363d; --accent:#58a6ff; --dim:#8b949e; }}
  [data-theme="light"] {{ --bg:#f5f5f0; --fg:#3a3530; --panel:rgba(255,255,255,0.92); --border:#d0ccc4; --accent:#3a6e96; --dim:#8a8478; }}
  body {{ background:var(--bg); overflow:hidden; font-family:monospace; color:var(--fg); transition:background 0.3s,color 0.3s; }}
  canvas {{ display:block; position:absolute; top:0; left:0; }}
  #svgOverlay {{ position:absolute; top:0; left:0; pointer-events:none; transition:opacity 0.2s; }}
  #svgOverlay.active {{ pointer-events:auto; }}
  #info {{ position:fixed; top:10px; left:10px; background:var(--panel); padding:10px 14px;
           border:1px solid var(--border); border-radius:6px; font-size:13px; z-index:10; transition:background 0.3s,border-color 0.3s; }}
  #info h3 {{ margin-bottom:6px; color:var(--accent); font-size:14px; }}
  #info div {{ margin:2px 0; }}
  #themeBtn {{ position:fixed; top:10px; right:10px; background:var(--panel); border:1px solid var(--border);
               border-radius:6px; padding:6px 12px; cursor:pointer; font-family:monospace; font-size:13px;
               color:var(--dim); z-index:10; transition:background 0.3s,border-color 0.3s,color 0.3s; }}
  #themeBtn:hover {{ color:var(--accent); border-color:var(--accent); }}
  #tooltip {{ position:fixed; display:none; background:var(--panel); padding:6px 10px;
              border:1px solid var(--accent); border-radius:4px; font-size:12px; pointer-events:none; z-index:20; }}
  #zoomLevel {{ color:var(--dim); }}
  .r {{ opacity:0.85; cursor:pointer; }}
  .r:hover {{ opacity:1; stroke:var(--accent); stroke-width:2; }}
</style>
<script>
(function(){{
  var t=localStorage.getItem('fabbula-theme');
  if(t) document.documentElement.setAttribute('data-theme',t);
}})();
</script></head><body>
<canvas id="canvas"></canvas>
<svg id="svgOverlay" xmlns="http://www.w3.org/2000/svg"></svg>
<button id="themeBtn" onclick="toggleTheme()">dark</button>
<div id="info">
  <h3>fabbula deep zoom</h3>
  <div>PDK: {pdk_name}</div>
  <div>Polygons: {poly_count}</div>
  <div>Size: {width_um:.1} x {height_um:.1} um</div>
  <div id="zoomLevel">Zoom: 1.0x</div>{legend}
  <div style="margin-top:6px;color:var(--dim)">Scroll to zoom, drag to pan</div>
</div>
<div id="tooltip"></div>
<script>
(function(){{
  const TILE_DIR='{tile_dir}';
  const DBU={dbu};
  var meta=null, polyData=null, polyLoading=false;
  var canvas=document.getElementById('canvas');
  var ctx=canvas.getContext('2d');
  var svgEl=document.getElementById('svgOverlay');
  var tooltip=document.getElementById('tooltip');
  var zoomInfo=document.getElementById('zoomLevel');

  // Viewport in artwork-normalized coords [0,1]
  var vp={{x:0, y:0, w:1, h:1}};
  var canvasW=window.innerWidth, canvasH=window.innerHeight;

  // Tile image cache
  var tileCache={{}};
  var SVG_POLY_THRESHOLD=5000;

  canvas.width=canvasW;
  canvas.height=canvasH;
  svgEl.setAttribute('width', canvasW);
  svgEl.setAttribute('height', canvasH);

  window.addEventListener('resize', function(){{
    canvasW=window.innerWidth; canvasH=window.innerHeight;
    canvas.width=canvasW; canvas.height=canvasH;
    svgEl.setAttribute('width', canvasW);
    svgEl.setAttribute('height', canvasH);
    render();
  }});

  // Load metadata
  fetch(TILE_DIR+'/meta.json').then(function(r){{ return r.json(); }}).then(function(m){{
    meta=m;
    // Set initial viewport to show full image with correct aspect ratio
    var imgAspect=meta.width/meta.height;
    var scrAspect=canvasW/canvasH;
    if(imgAspect>scrAspect){{
      vp.w=1; vp.h=imgAspect/scrAspect;
      vp.y=-(vp.h-1)/2;
    }} else {{
      vp.h=1; vp.w=scrAspect/imgAspect;
      vp.x=-(vp.w-1)/2;
    }}
    render();
  }});

  function getTileKey(level,col,row){{ return level+'/'+col+'_'+row; }}

  function loadTile(level,col,row){{
    var key=getTileKey(level,col,row);
    if(tileCache[key]) return tileCache[key];
    var img=new Image();
    img.onload=function(){{ img._loaded=true; render(); }};
    img.onerror=function(){{ img._failed=true; }};
    img.src=TILE_DIR+'/'+key+'.png';
    img._loaded=false;
    img._failed=false;
    tileCache[key]=img;
    return img;
  }}

  function render(){{
    if(!meta) return;
    ctx.clearRect(0,0,canvasW,canvasH);

    // Fill background
    var isDark=document.documentElement.getAttribute('data-theme')!=='light';
    ctx.fillStyle=isDark?'#0d1117':'#f5f5f0';
    ctx.fillRect(0,0,canvasW,canvasH);

    // Determine which tile level to use
    // vp.w represents the fraction of the full image visible horizontally
    // At vp.w=1, we see the full image, so we want the lowest detail level
    var pixelsPerViewport=meta.width/vp.w;
    var level=Math.max(0, Math.min(meta.maxLevel,
      Math.round(Math.log2(pixelsPerViewport/meta.tileSize))));

    // How many tiles at this level
    var levelScale=Math.pow(2, meta.maxLevel-level);
    var levelW=Math.ceil(meta.width/levelScale);
    var levelH=Math.ceil(meta.height/levelScale);
    var ts=meta.tileSize;
    var cols=Math.ceil(levelW/ts);
    var rows=Math.ceil(levelH/ts);

    // Tile size in normalized coords
    var tileNormW=ts*levelScale/meta.width;
    var tileNormH=ts*levelScale/meta.height;

    // Which tiles are visible
    var startCol=Math.max(0, Math.floor(vp.x/tileNormW));
    var endCol=Math.min(cols-1, Math.floor((vp.x+vp.w)/tileNormW));
    var startRow=Math.max(0, Math.floor(vp.y/tileNormH));
    var endRow=Math.min(rows-1, Math.floor((vp.y+vp.h)/tileNormH));

    var tileCount=0, MAX_TILES=256;
    for(var r=startRow;r<=endRow;r++){{
      for(var c=startCol;c<=endCol;c++){{
        if(++tileCount>MAX_TILES) break;
        var img=loadTile(level,c,r);
        // Tile position in normalized coords
        var tx=c*tileNormW;
        var ty=r*tileNormH;
        // Convert to screen coords
        var sx=(tx-vp.x)/vp.w*canvasW;
        var sy=(ty-vp.y)/vp.h*canvasH;
        var sw=tileNormW/vp.w*canvasW;
        var sh=tileNormH/vp.h*canvasH;

        if(img._loaded){{
          ctx.drawImage(img, sx, sy, sw, sh);
        }} else if(!img._failed){{
          // Try parent tile as fallback
          var pl=level-1, pc=Math.floor(c/2), pr=Math.floor(r/2);
          if(pl>=0){{
            var pimg=tileCache[getTileKey(pl,pc,pr)];
            if(pimg&&pimg._loaded){{
              // Draw the relevant quarter of the parent tile
              var subX=(c%2)*ts/2, subY=(r%2)*ts/2;
              ctx.drawImage(pimg, subX, subY, ts/2, ts/2, sx, sy, sw, sh);
            }}
          }}
        }}
      }}
    }}

    // Update zoom display
    var zoomFactor=(1/vp.w).toFixed(1);
    zoomInfo.textContent='Zoom: '+zoomFactor+'x | Level '+level+'/'+meta.maxLevel;

    // SVG overlay: check if we should show polygons
    updateSvgOverlay(level);
  }}

  function updateSvgOverlay(level){{
    if(!meta) return;
    // Estimate visible polygon density from densityGrid
    var grid=meta.densityGrid;
    var gs=grid.length;
    var cellW=1/gs, cellH=1/gs;
    var totalVisible=0;
    for(var gy=0;gy<gs;gy++){{
      for(var gx=0;gx<gs;gx++){{
        var cx=gx*cellW, cy=gy*cellH;
        // Check if this grid cell overlaps the viewport
        if(cx+cellW>vp.x && cx<vp.x+vp.w && cy+cellH>vp.y && cy<vp.y+vp.h){{
          totalVisible+=grid[gy][gx];
        }}
      }}
    }}

    if(totalVisible<SVG_POLY_THRESHOLD && totalVisible>0){{
      svgEl.style.opacity='1';
      svgEl.classList.add('active');
      loadAndRenderPolygons();
    }} else {{
      svgEl.style.opacity='0';
      svgEl.classList.remove('active');
    }}
  }}

  function loadAndRenderPolygons(){{
    if(polyData){{
      renderPolygons();
      return;
    }}
    if(polyLoading) return;
    polyLoading=true;
    fetch(TILE_DIR+'/polygons.json').then(function(r){{ return r.json(); }}).then(function(d){{
      polyData=d;
      renderPolygons();
    }});
  }}

  function renderPolygons(){{
    if(!polyData||!meta) return;
    var ox=polyData.ox, oy=polyData.oy;
    var bbW=polyData.bb_w, bbH=polyData.bb_h;
    var html='';

    for(var li=0;li<polyData.layers.length;li++){{
      var layer=polyData.layers[li];
      var rects=layer.rects;
      var color=layer.color;
      var name=layer.name;

      for(var i=0;i<rects.length;i++){{
        var r=rects[i];
        // r = [x0_rel, y0_rel, x1_rel, y1_rel] relative to bb origin
        // Convert to normalized [0,1] coords then to screen
        var nx0=r[0]/bbW, ny0=r[1]/bbH;
        var nx1=r[2]/bbW, ny1=r[3]/bbH;
        // Flip Y: in artwork coords y goes up, in screen coords y goes down
        var sy0=(1-ny1), sy1=(1-ny0);
        // Convert to screen pixels
        var sx=(nx0-vp.x)/vp.w*canvasW;
        var sy2=(sy0-vp.y)/vp.h*canvasH;
        var sw=(nx1-nx0)/vp.w*canvasW;
        var sh=(sy1-sy0)/vp.h*canvasH;

        // Skip if not visible or too small
        if(sx+sw<0||sx>canvasW||sy2+sh<0||sy2>canvasH) continue;
        if(sw<1&&sh<1) continue;

        // Actual coordinates for tooltip
        var ax0=r[0]+ox, ay0=r[1]+oy, ax1=r[2]+ox, ay1=r[3]+oy;
        html+='<rect x="'+sx.toFixed(1)+'" y="'+sy2.toFixed(1)+'" width="'+sw.toFixed(1)+'" height="'+sh.toFixed(1)+'" fill="'+color+'" class="r" data-x0="'+ax0+'" data-y0="'+ay0+'" data-x1="'+ax1+'" data-y1="'+ay1+'" data-layer="'+name+'"/>';
      }}
    }}
    svgEl.innerHTML=html;
  }}

  // Zoom with wheel
  var MIN_VP=0.001; // max zoom ~1000x
  var MAX_VP=10;    // max zoom out ~10x
  canvas.addEventListener('wheel', function(e){{
    e.preventDefault();
    var s=e.deltaY>0?1.15:1/1.15;
    var nw=vp.w*s, nh=vp.h*s;
    if(nw<MIN_VP||nh<MIN_VP||nw>MAX_VP||nh>MAX_VP) return;
    var mx=e.clientX/canvasW, my=e.clientY/canvasH;
    var artX=vp.x+mx*vp.w;
    var artY=vp.y+my*vp.h;
    vp.w=nw; vp.h=nh;
    vp.x=artX-mx*vp.w;
    vp.y=artY-my*vp.h;
    render();
  }});

  // Pan with mouse drag
  var dragging=false, lastMX, lastMY;
  canvas.addEventListener('mousedown', function(e){{
    dragging=true; lastMX=e.clientX; lastMY=e.clientY;
    canvas.style.cursor='grabbing';
  }});
  window.addEventListener('mousemove', function(e){{
    if(!dragging) return;
    var dx=(e.clientX-lastMX)/canvasW*vp.w;
    var dy=(e.clientY-lastMY)/canvasH*vp.h;
    vp.x-=dx; vp.y-=dy;
    lastMX=e.clientX; lastMY=e.clientY;
    render();
  }});
  window.addEventListener('mouseup', function(){{
    dragging=false; canvas.style.cursor='default';
  }});

  // Tooltip for SVG overlay
  svgEl.addEventListener('mouseover', function(e){{
    var t=e.target;
    if(!t.classList.contains('r')) return;
    var x0=(+t.dataset.x0/DBU).toFixed(3);
    var y0=(+t.dataset.y0/DBU).toFixed(3);
    var x1=(+t.dataset.x1/DBU).toFixed(3);
    var y1=(+t.dataset.y1/DBU).toFixed(3);
    var w=((t.dataset.x1-t.dataset.x0)/DBU).toFixed(3);
    var h=((t.dataset.y1-t.dataset.y0)/DBU).toFixed(3);
    var layer=t.dataset.layer||'';
    tooltip.innerHTML=(layer?layer+': ':'')+'('+x0+', '+y0+') - ('+x1+', '+y1+') um<br>'+w+' x '+h+' um';
    tooltip.style.display='block';
  }});
  svgEl.addEventListener('mousemove', function(e){{
    if(tooltip.style.display==='block'){{ tooltip.style.left=(e.clientX+12)+'px'; tooltip.style.top=(e.clientY+12)+'px'; }}
  }});
  svgEl.addEventListener('mouseout', function(e){{
    if(e.target.classList.contains('r')) tooltip.style.display='none';
  }});

  // Theme toggle
  window.toggleTheme=function(){{
    var r=document.documentElement;
    var c=r.getAttribute('data-theme')==='light'?'dark':'light';
    r.setAttribute('data-theme',c);
    localStorage.setItem('fabbula-theme',c);
    document.getElementById('themeBtn').textContent=c;
    render();
  }};

  // Apply saved theme
  var th=localStorage.getItem('fabbula-theme')||'dark';
  document.getElementById('themeBtn').textContent=th;
}})();
</script></body></html>
"##,
        pdk_name = pdk.pdk.name,
        poly_count = total_polys,
        width_um = width_um,
        height_um = height_um,
        legend = legend_html,
        tile_dir = tile_dir_rel,
        dbu = dbu,
    )?;

    tracing::info!(
        "Wrote deep zoom HTML preview: {} ({} polygons, {} layers)",
        output.display(),
        total_polys,
        layers.len()
    );

    Ok(output.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdk::PdkConfig;
    use crate::polygon::Rect;

    fn sample_rects() -> Vec<Rect> {
        vec![
            Rect::new(0, 0, 100, 100),
            Rect::new(200, 0, 300, 100),
            Rect::new(0, 200, 100, 300),
        ]
    }

    #[test]
    fn test_write_svg_single_layer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.svg");
        let rects = sample_rects();

        write_svg(&rects, &path, 0.01, "#c0c0c0", None).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("xmlns=\"http://www.w3.org/2000/svg\""),
            "SVG should contain xmlns declaration"
        );
        assert!(
            content.contains("viewBox="),
            "SVG should contain viewBox attribute"
        );
        assert!(
            content.contains("<rect"),
            "SVG should contain at least one rect element"
        );
        assert!(
            content.contains("fill=\"#c0c0c0\""),
            "SVG should contain the specified fill color"
        );
        assert!(content.contains("</svg>"), "SVG should have a closing tag");
    }

    #[test]
    fn test_write_svg_multi_layer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("multi.svg");
        let rects_a = vec![Rect::new(0, 0, 100, 100)];
        let rects_b = vec![Rect::new(200, 200, 300, 300)];

        let layers = vec![
            SvgLayer {
                rects: &rects_a,
                color: "#ff0000",
            },
            SvgLayer {
                rects: &rects_b,
                color: "#00ff00",
            },
        ];

        write_svg_multi(&layers, &path, 0.01, None).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("fill=\"#ff0000\""),
            "SVG should contain the first layer color"
        );
        assert!(
            content.contains("fill=\"#00ff00\""),
            "SVG should contain the second layer color"
        );
    }

    #[test]
    fn test_write_svg_with_background() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bg.svg");
        let rects = sample_rects();

        write_svg(&rects, &path, 0.01, "#c0c0c0", Some("#1a1a2e")).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("fill=\"#1a1a2e\""),
            "SVG should contain the background fill color"
        );
        // The background rect appears before the transform group
        let bg_pos = content.find("fill=\"#1a1a2e\"").unwrap();
        let group_pos = content.find("<g transform=").unwrap();
        assert!(
            bg_pos < group_pos,
            "Background rect should appear before the artwork group"
        );
    }

    #[test]
    fn test_write_svg_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.svg");
        let rects: Vec<Rect> = vec![];

        write_svg(&rects, &path, 0.01, "#c0c0c0", None).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("viewBox="),
            "Empty SVG should still have a viewBox (fallback bounding box)"
        );
        assert!(
            content.contains("xmlns=\"http://www.w3.org/2000/svg\""),
            "Empty SVG should still be a valid SVG document"
        );
        assert!(
            content.contains("</svg>"),
            "Empty SVG should have a closing tag"
        );
    }

    #[test]
    fn test_write_html_preview_single() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preview.html");
        let rects = sample_rects();
        let pdk = PdkConfig::builtin("sky130").unwrap();

        write_html_preview(&rects, &path, &pdk).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("<svg") || content.contains("<canvas"),
            "HTML should contain an svg or canvas element"
        );
        assert!(
            content.contains("PDK: sky130"),
            "HTML should display the PDK name"
        );
        assert!(
            content.contains("Polygons: 3"),
            "HTML should display the polygon count"
        );
        assert!(
            content.contains("fabbula preview"),
            "HTML should contain the fabbula preview heading"
        );
    }

    #[test]
    fn test_write_html_preview_multi() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("multi.html");
        let rects_a = vec![Rect::new(0, 0, 100, 100)];
        let rects_b = vec![Rect::new(200, 200, 300, 300)];
        let pdk = PdkConfig::builtin("sky130").unwrap();

        let layers = vec![
            HtmlLayer {
                rects: &rects_a,
                name: "metal5",
                color: "#ff0000",
            },
            HtmlLayer {
                rects: &rects_b,
                name: "metal4",
                color: "#00ff00",
            },
        ];

        write_html_preview_multi(&layers, &path, &pdk).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("Layers:"),
            "Multi-layer HTML should contain a layer legend"
        );
        assert!(
            content.contains("metal5"),
            "Legend should list the first layer name"
        );
        assert!(
            content.contains("metal4"),
            "Legend should list the second layer name"
        );
        assert!(
            content.contains("#ff0000"),
            "Legend should include the first layer color"
        );
        assert!(
            content.contains("#00ff00"),
            "Legend should include the second layer color"
        );
    }
}
