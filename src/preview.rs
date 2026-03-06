use crate::pdk::PdkConfig;
use crate::polygon::{bounding_box, Rect};
use anyhow::{Context, Result};
use std::io::{BufWriter, Write};
use std::path::Path;

/// Write an SVG preview of the generated polygons
pub fn write_svg(
    rects: &[Rect],
    output: &Path,
    scale: f64,
    fill_color: &str,
    background: Option<&str>,
) -> Result<()> {
    let bb = bounding_box(rects).unwrap_or(Rect::new(0, 0, 1000, 1000));

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

    // Flip Y axis: SVG has Y going down, GDS has Y going up
    writeln!(
        f,
        r#"  <g transform="translate(0, {}) scale(1, -1)">"#,
        vb_y * 2 + vb_h
    )?;

    for rect in rects {
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

    writeln!(f, "  </g>")?;
    writeln!(f, "</svg>")?;

    tracing::info!(
        "Wrote SVG preview: {} ({}x{})",
        output.display(),
        svg_w,
        svg_h
    );
    Ok(())
}

/// Write a self-contained interactive HTML preview with pan, zoom, and hover tooltips.
/// Streams rects directly to file instead of building an intermediate String.
pub fn write_html_preview(rects: &[Rect], output: &Path, pdk: &PdkConfig) -> Result<()> {
    let bb = bounding_box(rects).unwrap_or(Rect::new(0, 0, 1000, 1000));
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
  <div style="margin-top:6px;color:#8b949e">Scroll to zoom, drag to pan</div>
</div>
<div id="tooltip"></div>
<svg id="canvas" xmlns="http://www.w3.org/2000/svg" width="100%" height="100%"
     viewBox="{vb_x} {vb_y} {vb_w} {vb_h}">
  <rect x="{vb_x}" y="{vb_y}" width="{vb_w}" height="{vb_h}" fill="#0d1117"/>
  <g id="art" transform="translate(0, {flip_y}) scale(1, -1)">
"##,
        pdk_name = pdk.pdk.name,
        poly_count = rects.len(),
        width_um = width_um,
        height_um = height_um,
        vb_x = vb_x,
        vb_y = vb_y,
        vb_w = vb_w,
        vb_h = vb_h,
        flip_y = flip_y,
        sw = sw,
    )?;

    // Part 2: Stream rects directly
    for rect in rects {
        writeln!(
            f,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#b0c4de\" class=\"r\" \
             data-x0=\"{}\" data-y0=\"{}\" data-x1=\"{}\" data-y1=\"{}\"/>",
            rect.x0,
            rect.y0,
            rect.width(),
            rect.height(),
            rect.x0,
            rect.y0,
            rect.x1,
            rect.y1
        )?;
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
    tooltip.innerHTML='('+x0+', '+y0+') - ('+x1+', '+y1+') um<br>'+w+' x '+h+' um';
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
        "Wrote HTML preview: {} ({} polygons)",
        output.display(),
        rects.len()
    );
    Ok(())
}
