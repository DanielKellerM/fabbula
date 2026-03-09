// Copyright 2026 Daniel Keller <daniel.keller.m@gmail.com>
// Licensed under the Apache License, Version 2.0.
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_arch = "wasm32")]

use crate::pdk::BuiltinPdk;
use crate::polygon::{Rect, bounding_box};
use crate::wasm_app::{GenerateRequest, generate_gds_bytes, run_pipeline, validate_pdk_toml};
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;
use web_sys::{
    Blob, CanvasRenderingContext2d, Document, Event, File, FileReader, HtmlAnchorElement,
    HtmlButtonElement, HtmlCanvasElement, HtmlElement, HtmlImageElement, HtmlInputElement,
    HtmlSelectElement, HtmlTextAreaElement, ImageData, KeyboardEvent, MouseEvent, Url, WheelEvent,
    Window,
};

const WARN_UPLOAD_BYTES: u64 = 200 * 1024 * 1024;
const BLOCK_UPLOAD_BYTES: u64 = 512 * 1024 * 1024;
const AUTO_PREVIEW_MAX_DIM: u32 = 384;
const CUSTOM_PDK_STORAGE_KEY: &str = "fabbula.custom_pdk_toml";

#[derive(Debug, Clone)]
struct ViewState {
    zoom: f64,
    pan_x: f64,
    pan_y: f64,
    dragging: bool,
    last_x: f64,
    last_y: f64,
}

type RgbaFrame = (Vec<u8>, u32, u32);
type SharedRgbaState = std::rc::Rc<std::cell::RefCell<Option<RgbaFrame>>>;

impl Default for ViewState {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
            dragging: false,
            last_x: 0.0,
            last_y: 0.0,
        }
    }
}

fn window() -> Result<Window, JsValue> {
    web_sys::window().ok_or_else(|| JsValue::from_str("window not available"))
}

fn document() -> Result<Document, JsValue> {
    window()?
        .document()
        .ok_or_else(|| JsValue::from_str("document not available"))
}

fn set_status(el: &HtmlElement, msg: &str) {
    el.set_text_content(Some(msg));
}

fn optional_text(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn parse_u32_input(input: &HtmlInputElement, default: u32, min: u32, max: u32) -> u32 {
    input
        .value()
        .trim()
        .parse::<u32>()
        .ok()
        .map(|v| v.clamp(min, max))
        .unwrap_or(default)
}

fn parse_query(search: &str) -> Vec<(String, String)> {
    let s = search.strip_prefix('?').unwrap_or(search);
    if s.is_empty() {
        return Vec::new();
    }
    s.split('&')
        .filter_map(|part| {
            let mut it = part.splitn(2, '=');
            let k = it.next()?.trim();
            if k.is_empty() {
                return None;
            }
            let v = it.next().unwrap_or("").trim();
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

fn demo_rgba(kind: &str, width: u32, height: u32) -> Vec<u8> {
    let mut px = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let i = ((y * width + x) * 4) as usize;
            let (r, g, b) = match kind {
                "checker" => {
                    let c = if ((x / 24) + (y / 24)) % 2 == 0 {
                        235
                    } else {
                        35
                    };
                    (c, c, c)
                }
                "rings" => {
                    let cx = width as f64 * 0.5;
                    let cy = height as f64 * 0.5;
                    let dx = x as f64 - cx;
                    let dy = y as f64 - cy;
                    let d = (dx * dx + dy * dy).sqrt();
                    let c = if ((d / 14.0) as i32) % 2 == 0 {
                        245
                    } else {
                        30
                    };
                    (c, c, c)
                }
                _ => {
                    let gx = ((x as f64 / (width.max(1) as f64 - 1.0)) * 255.0) as u8;
                    let gy = ((y as f64 / (height.max(1) as f64 - 1.0)) * 255.0) as u8;
                    (gx, gy, 255u8.saturating_sub(gx / 2))
                }
            };
            px[i] = r;
            px[i + 1] = g;
            px[i + 2] = b;
            px[i + 3] = 255;
        }
    }
    px
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * KB;
    const GB: f64 = 1024.0 * MB;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

fn preview_transform(
    canvas_w: f64,
    canvas_h: f64,
    rects: &[Rect],
    zoom: f64,
) -> Option<(f64, f64, f64)> {
    if rects.is_empty() {
        return None;
    }
    let bb = bounding_box(rects).unwrap_or(Rect::new(0, 0, 1, 1));
    let w = (bb.width().0 as f64).max(1.0);
    let h = (bb.height().0 as f64).max(1.0);
    let base_scale = (canvas_w / w).min(canvas_h / h) * 0.95;
    let scale = base_scale * zoom;
    let ox = (canvas_w - w * scale) * 0.5 - bb.x0.0 as f64 * scale;
    let oy = (canvas_h - h * scale) * 0.5 - bb.y0.0 as f64 * scale;
    Some((scale, ox, oy))
}

fn draw_preview(
    ctx: &CanvasRenderingContext2d,
    canvas_w: f64,
    canvas_h: f64,
    rects: &[Rect],
    view: &ViewState,
) {
    ctx.set_fill_style_str("var(--canvas-bg)");
    ctx.fill_rect(0.0, 0.0, canvas_w, canvas_h);
    if rects.is_empty() {
        return;
    }
    let Some((scale, ox, oy)) = preview_transform(canvas_w, canvas_h, rects, view.zoom) else {
        return;
    };

    ctx.set_fill_style_str("var(--gh-text)");
    for r in rects {
        let x = ox + r.x0.0 as f64 * scale + view.pan_x;
        let y = oy + r.y0.0 as f64 * scale;
        let rw = (r.width().0 as f64).max(1.0) * scale;
        let rh = (r.height().0 as f64).max(1.0) * scale;
        ctx.fill_rect(x, canvas_h - (y + rh) + view.pan_y, rw, rh);
    }
}

fn downsample_rgba_nearest(
    pixels: &[u8],
    width: u32,
    height: u32,
    max_dim: u32,
) -> (Vec<u8>, u32, u32) {
    if width == 0 || height == 0 {
        return (Vec::new(), width, height);
    }
    let max_wh = width.max(height);
    if max_wh <= max_dim {
        return (pixels.to_vec(), width, height);
    }

    let scale = max_dim as f64 / max_wh as f64;
    let out_w = ((width as f64 * scale).round() as u32).max(1);
    let out_h = ((height as f64 * scale).round() as u32).max(1);
    let mut out = vec![0u8; (out_w * out_h * 4) as usize];
    for y in 0..out_h {
        let src_y = ((y as f64 / out_h as f64) * height as f64).floor() as u32;
        let sy = src_y.min(height - 1);
        for x in 0..out_w {
            let src_x = ((x as f64 / out_w as f64) * width as f64).floor() as u32;
            let sx = src_x.min(width - 1);
            let src_i = ((sy * width + sx) * 4) as usize;
            let dst_i = ((y * out_w + x) * 4) as usize;
            out[dst_i..dst_i + 4].copy_from_slice(&pixels[src_i..src_i + 4]);
        }
    }
    (out, out_w, out_h)
}

pub(crate) fn start_app() -> Result<(), JsValue> {
    use std::cell::RefCell;
    use std::rc::Rc;

    let doc = document()?;
    let body = doc
        .body()
        .ok_or_else(|| JsValue::from_str("document has no body"))?;

    let container = doc.create_element("div")?;
    container.set_attribute(
        "style",
        "max-width:1180px;margin:24px auto;padding:20px;color:var(--gh-text);background:var(--gh-surface);font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Helvetica,Arial,sans-serif;border:1px solid var(--gh-border);border-radius:12px;box-shadow:0 1px 0 rgba(27,31,36,0.04);",
    )?;
    body.append_child(&container)?;

    let title = doc.create_element("h2")?;
    title.set_text_content(Some("fabbula wasm"));
    container.append_child(&title)?;

    let controls = doc.create_element("div")?;
    controls.set_attribute(
        "style",
        "display:flex;gap:8px;flex-wrap:wrap;align-items:center;margin-bottom:14px;",
    )?;
    container.append_child(&controls)?;

    let file_input: HtmlInputElement = doc.create_element("input")?.dyn_into()?;
    file_input.set_type("file");
    file_input.set_accept("image/*");
    controls.append_child(&file_input)?;

    let pdk_select: HtmlSelectElement = doc.create_element("select")?.dyn_into()?;
    for name in [
        "sky130",
        "ihp_sg13g2",
        "gf180mcu",
        "freepdk45",
        "asap7",
        "fabbula2",
    ] {
        let opt = doc.create_element("option")?;
        opt.set_text_content(Some(name));
        opt.set_attribute("value", name)?;
        pdk_select.append_child(&opt)?;
    }
    controls.append_child(&pdk_select)?;

    let strategy_select: HtmlSelectElement = doc.create_element("select")?.dyn_into()?;
    for (label, value) in [
        ("greedy-merge", "greedy-merge"),
        ("histogram-merge", "histogram-merge"),
        ("row-merge", "row-merge"),
        ("pixel-rects", "pixel-rects"),
    ] {
        let opt = doc.create_element("option")?;
        opt.set_text_content(Some(label));
        opt.set_attribute("value", value)?;
        strategy_select.append_child(&opt)?;
    }
    controls.append_child(&strategy_select)?;

    let invert_label: HtmlElement = doc.create_element("label")?.dyn_into()?;
    invert_label.set_text_content(Some("Invert"));
    invert_label.set_attribute("style", "display:flex;align-items:center;gap:4px;")?;
    let invert_toggle: HtmlInputElement = doc.create_element("input")?.dyn_into()?;
    invert_toggle.set_type("checkbox");
    invert_label.append_child(&invert_toggle)?;
    controls.append_child(&invert_label)?;

    let dither_label: HtmlElement = doc.create_element("label")?.dyn_into()?;
    dither_label.set_text_content(Some("Dither"));
    dither_label.set_attribute("style", "display:flex;align-items:center;gap:4px;")?;
    let dither_toggle: HtmlInputElement = doc.create_element("input")?.dyn_into()?;
    dither_toggle.set_type("checkbox");
    dither_label.append_child(&dither_toggle)?;
    controls.append_child(&dither_label)?;

    let custom_pdk_label: HtmlElement = doc.create_element("label")?.dyn_into()?;
    custom_pdk_label.set_text_content(Some("Custom PDK"));
    custom_pdk_label.set_attribute("style", "display:flex;align-items:center;gap:4px;")?;
    let custom_pdk_toggle: HtmlInputElement = doc.create_element("input")?.dyn_into()?;
    custom_pdk_toggle.set_type("checkbox");
    custom_pdk_label.append_child(&custom_pdk_toggle)?;
    controls.append_child(&custom_pdk_label)?;

    let text_input: HtmlInputElement = doc.create_element("input")?.dyn_into()?;
    text_input.set_attribute("placeholder", "Text overlay")?;
    controls.append_child(&text_input)?;

    let text_pos_select: HtmlSelectElement = doc.create_element("select")?.dyn_into()?;
    for value in [
        "bottom",
        "top",
        "center",
        "top-left",
        "top-right",
        "bottom-left",
        "bottom-right",
    ] {
        let opt = doc.create_element("option")?;
        opt.set_text_content(Some(value));
        opt.set_attribute("value", value)?;
        text_pos_select.append_child(&opt)?;
    }
    text_pos_select.set_value("bottom");
    controls.append_child(&text_pos_select)?;

    let text_scale: HtmlInputElement = doc.create_element("input")?.dyn_into()?;
    text_scale.set_type("number");
    text_scale.set_attribute("min", "1")?;
    text_scale.set_attribute("max", "32")?;
    text_scale.set_value("1");
    text_scale.set_attribute("title", "Text scale")?;
    controls.append_child(&text_scale)?;

    let qr_input: HtmlInputElement = doc.create_element("input")?.dyn_into()?;
    qr_input.set_attribute("placeholder", "QR data")?;
    controls.append_child(&qr_input)?;

    let qr_pos_select: HtmlSelectElement = doc.create_element("select")?.dyn_into()?;
    for value in [
        "bottom-right",
        "bottom-left",
        "top-right",
        "top-left",
        "center",
        "bottom",
        "top",
    ] {
        let opt = doc.create_element("option")?;
        opt.set_text_content(Some(value));
        opt.set_attribute("value", value)?;
        qr_pos_select.append_child(&opt)?;
    }
    qr_pos_select.set_value("bottom-right");
    controls.append_child(&qr_pos_select)?;

    let qr_module_size: HtmlInputElement = doc.create_element("input")?.dyn_into()?;
    qr_module_size.set_type("number");
    qr_module_size.set_attribute("min", "1")?;
    qr_module_size.set_attribute("max", "32")?;
    qr_module_size.set_value("2");
    qr_module_size.set_attribute("title", "QR module size")?;
    controls.append_child(&qr_module_size)?;

    let qr_ec_select: HtmlSelectElement = doc.create_element("select")?.dyn_into()?;
    for value in ["l", "m", "q", "h"] {
        let opt = doc.create_element("option")?;
        opt.set_text_content(Some(value));
        opt.set_attribute("value", value)?;
        qr_ec_select.append_child(&opt)?;
    }
    qr_ec_select.set_value("m");
    controls.append_child(&qr_ec_select)?;

    let overlay_margin: HtmlInputElement = doc.create_element("input")?.dyn_into()?;
    overlay_margin.set_type("number");
    overlay_margin.set_attribute("min", "0")?;
    overlay_margin.set_attribute("max", "256")?;
    overlay_margin.set_value("2");
    overlay_margin.set_attribute("title", "Overlay margin px")?;
    controls.append_child(&overlay_margin)?;

    let generate_btn: HtmlButtonElement = doc.create_element("button")?.dyn_into()?;
    generate_btn.set_text_content(Some("Generate"));
    controls.append_child(&generate_btn)?;

    let download_btn: HtmlButtonElement = doc.create_element("button")?.dyn_into()?;
    download_btn.set_text_content(Some("Download GDS"));
    download_btn.set_disabled(true);
    controls.append_child(&download_btn)?;

    let canvas: HtmlCanvasElement = doc.create_element("canvas")?.dyn_into()?;
    canvas.set_width(1024);
    canvas.set_height(768);
    canvas.set_attribute(
        "style",
        "width:100%;height:auto;border:1px solid var(--gh-border);background:var(--canvas-bg);cursor:grab;border-radius:8px;",
    )?;
    container.append_child(&canvas)?;
    let ctx: CanvasRenderingContext2d = canvas
        .get_context("2d")?
        .ok_or_else(|| JsValue::from_str("no 2d context"))?
        .dyn_into()?;

    let stats: HtmlElement = doc.create_element("div")?.dyn_into()?;
    stats.set_attribute(
        "style",
        "margin-top:12px;padding:10px;background:var(--gh-subtle);border:1px solid var(--gh-border);border-radius:8px;color:var(--gh-text);",
    )?;
    stats.set_text_content(Some("Stats: waiting for generation."));
    container.append_child(&stats)?;

    let drc_badge: HtmlElement = doc.create_element("div")?.dyn_into()?;
    drc_badge.set_attribute(
        "style",
        "margin-top:8px;padding:8px;border-radius:8px;background:var(--ok-bg);color:var(--ok-fg);border:1px solid var(--ok-border);",
    )?;
    drc_badge.set_text_content(Some("DRC: n/a"));
    container.append_child(&drc_badge)?;

    let drc_details: HtmlElement = doc.create_element("pre")?.dyn_into()?;
    drc_details.set_attribute(
        "style",
        "white-space:pre-wrap;background:var(--gh-subtle);padding:10px;border:1px solid var(--gh-border);border-radius:8px;margin-top:8px;max-height:220px;overflow:auto;color:var(--gh-text);",
    )?;
    drc_details.set_text_content(Some("No violations."));
    container.append_child(&drc_details)?;

    let pdk_editor_wrap: HtmlElement = doc.create_element("div")?.dyn_into()?;
    pdk_editor_wrap.set_attribute(
        "style",
        "display:none;margin-top:10px;padding:10px;background:var(--gh-subtle);border:1px solid var(--gh-border);border-radius:8px;",
    )?;
    container.append_child(&pdk_editor_wrap)?;

    let pdk_editor_row: HtmlElement = doc.create_element("div")?.dyn_into()?;
    pdk_editor_row.set_attribute(
        "style",
        "display:flex;gap:8px;align-items:center;margin-bottom:8px;",
    )?;
    pdk_editor_wrap.append_child(&pdk_editor_row)?;

    let template_select: HtmlSelectElement = doc.create_element("select")?.dyn_into()?;
    for name in [
        "sky130",
        "ihp_sg13g2",
        "gf180mcu",
        "freepdk45",
        "asap7",
        "fabbula2",
    ] {
        let opt = doc.create_element("option")?;
        opt.set_text_content(Some(name));
        opt.set_attribute("value", name)?;
        template_select.append_child(&opt)?;
    }
    template_select.set_value(&pdk_select.value());
    pdk_editor_row.append_child(&template_select)?;

    let load_template_btn: HtmlButtonElement = doc.create_element("button")?.dyn_into()?;
    load_template_btn.set_text_content(Some("Use Template"));
    pdk_editor_row.append_child(&load_template_btn)?;

    let upload_toml_btn: HtmlButtonElement = doc.create_element("button")?.dyn_into()?;
    upload_toml_btn.set_text_content(Some("Upload .toml"));
    pdk_editor_row.append_child(&upload_toml_btn)?;

    let upload_toml_input: HtmlInputElement = doc.create_element("input")?.dyn_into()?;
    upload_toml_input.set_type("file");
    upload_toml_input.set_accept(".toml,text/plain");
    upload_toml_input.set_attribute("style", "display:none;")?;
    pdk_editor_row.append_child(&upload_toml_input)?;

    let pdk_validation: HtmlElement = doc.create_element("div")?.dyn_into()?;
    pdk_validation.set_attribute(
        "style",
        "padding:4px 8px;border-radius:6px;background:var(--err-bg);color:var(--err-fg);border:1px solid var(--err-border);",
    )?;
    pdk_validation.set_text_content(Some("Custom PDK not validated yet."));
    pdk_editor_row.append_child(&pdk_validation)?;

    let pdk_editor: HtmlTextAreaElement = doc.create_element("textarea")?.dyn_into()?;
    pdk_editor.set_attribute(
        "style",
        "width:100%;min-height:220px;background:var(--gh-surface);color:var(--gh-text);border:1px solid var(--gh-border);border-radius:8px;padding:10px;font:12px/1.5 ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;",
    )?;
    pdk_editor_wrap.append_child(&pdk_editor)?;

    let status: HtmlElement = doc.create_element("pre")?.dyn_into()?;
    status.set_attribute(
        "style",
        "white-space:pre-wrap;background:var(--gh-subtle);padding:10px;border:1px solid var(--gh-border);border-radius:8px;margin-top:12px;color:var(--gh-text);",
    )?;
    set_status(
        &status,
        "Load an image, then click Generate. Use wheel to zoom and drag to pan.",
    );
    container.append_child(&status)?;

    let rgba_state: SharedRgbaState = Rc::new(RefCell::new(None));
    let last_req: Rc<RefCell<Option<GenerateRequest>>> = Rc::new(RefCell::new(None));
    let last_rects: Rc<RefCell<Vec<Rect>>> = Rc::new(RefCell::new(Vec::new()));
    let view_state: Rc<RefCell<ViewState>> = Rc::new(RefCell::new(ViewState::default()));
    let debounce_timer: Rc<RefCell<Option<i32>>> = Rc::new(RefCell::new(None));
    let mut initial_pdk_from_url: Option<String> = None;
    let mut load_demo_from_url: Option<String> = None;

    if let Ok(search) = window()?.location().search() {
        for (k, v) in parse_query(&search) {
            if k.eq_ignore_ascii_case("pdk") {
                initial_pdk_from_url = Some(v);
            } else if k.eq_ignore_ascii_case("demo") {
                load_demo_from_url = Some(v);
            }
        }
    }
    if let Some(pdk) = initial_pdk_from_url
        && pdk.parse::<BuiltinPdk>().is_ok()
    {
        pdk_select.set_value(&pdk);
        template_select.set_value(&pdk);
    }
    if let Ok(Some(storage)) = window()?.local_storage()
        && let Ok(Some(saved)) = storage.get_item(CUSTOM_PDK_STORAGE_KEY)
        && !saved.trim().is_empty()
    {
        pdk_editor.set_value(&saved);
    }

    {
        let rgba_state = Rc::clone(&rgba_state);
        let status_el = status.clone();
        let file_input_for_change = file_input.clone();
        let doc = doc.clone();
        let onchange = Closure::<dyn FnMut(Event)>::new(move |_| {
            let Some(files) = file_input_for_change.files() else {
                set_status(&status_el, "No file selected.");
                return;
            };
            let Some(file): Option<File> = files.item(0) else {
                set_status(&status_el, "No file selected.");
                return;
            };
            let size = file.size() as u64;
            if size > BLOCK_UPLOAD_BYTES {
                set_status(
                    &status_el,
                    &format!(
                        "File too large ({}). Browser mode keeps data in memory and may freeze/OOM on very large files. Use CLI for huge inputs.",
                        format_bytes(size)
                    ),
                );
                return;
            }
            if size > WARN_UPLOAD_BYTES {
                set_status(
                    &status_el,
                    &format!(
                        "Large file detected ({}). Browser mode duplicates buffers (file decode + RGBA + WASM memory), so performance may degrade.",
                        format_bytes(size)
                    ),
                );
            }
            let Ok(url) = Url::create_object_url_with_blob(&file) else {
                set_status(&status_el, "Failed to create object URL.");
                return;
            };
            let Ok(img) = HtmlImageElement::new() else {
                set_status(&status_el, "Failed to create image element.");
                let _ = Url::revoke_object_url(&url);
                return;
            };
            let img_clone = img.clone();
            let url_clone = url.clone();
            let status_inner = status_el.clone();
            let rgba_state = Rc::clone(&rgba_state);
            let doc = doc.clone();
            let onload = Closure::<dyn FnMut()>::new(move || {
                let w = img_clone.width();
                let h = img_clone.height();
                let Ok(scratch): Result<HtmlCanvasElement, _> = doc
                    .create_element("canvas")
                    .and_then(|e| e.dyn_into::<HtmlCanvasElement>().map_err(Into::into))
                else {
                    set_status(&status_inner, "Failed to create scratch canvas.");
                    let _ = Url::revoke_object_url(&url_clone);
                    return;
                };
                scratch.set_width(w);
                scratch.set_height(h);
                let Ok(ctx): Result<CanvasRenderingContext2d, _> = scratch
                    .get_context("2d")
                    .and_then(|o| o.ok_or_else(|| JsValue::from_str("no context")))
                    .and_then(|v| v.dyn_into::<CanvasRenderingContext2d>().map_err(Into::into))
                else {
                    set_status(&status_inner, "Failed to get scratch 2d context.");
                    let _ = Url::revoke_object_url(&url_clone);
                    return;
                };
                if ctx
                    .draw_image_with_html_image_element(&img_clone, 0.0, 0.0)
                    .is_err()
                {
                    set_status(&status_inner, "Failed to draw image.");
                    let _ = Url::revoke_object_url(&url_clone);
                    return;
                }
                let Ok(image_data): Result<ImageData, _> =
                    ctx.get_image_data(0.0, 0.0, w as f64, h as f64)
                else {
                    set_status(&status_inner, "Failed to read image pixels.");
                    let _ = Url::revoke_object_url(&url_clone);
                    return;
                };
                let pixels = image_data.data().to_vec();
                *rgba_state.borrow_mut() = Some((pixels, w, h));
                set_status(
                    &status_inner,
                    &format!("Loaded image: {}x{} px. Click Generate.", w, h),
                );
                let _ = Url::revoke_object_url(&url_clone);
            });
            img.set_onload(Some(onload.as_ref().unchecked_ref()));
            onload.forget();
            img.set_src(&url);
        });
        file_input.set_onchange(Some(onchange.as_ref().unchecked_ref()));
        onchange.forget();
    }

    {
        let pdk_editor_wrap = pdk_editor_wrap.clone();
        let pdk_validation = pdk_validation.clone();
        let pdk_editor = pdk_editor.clone();
        let template_select = template_select.clone();
        let custom_pdk_toggle = custom_pdk_toggle.clone();
        let custom_pdk_toggle_for_handler = custom_pdk_toggle.clone();
        let pdk_select = pdk_select.clone();
        let ontoggle = Closure::<dyn FnMut(Event)>::new(move |_| {
            if custom_pdk_toggle_for_handler.checked() {
                pdk_select.set_disabled(true);
                let _ = pdk_editor_wrap.set_attribute(
                    "style",
                    "display:block;margin-top:10px;padding:10px;background:var(--gh-subtle);border:1px solid var(--gh-border);border-radius:8px;",
                );
                if pdk_editor.value().trim().is_empty()
                    && let Ok(builtin) = pdk_select.value().parse::<BuiltinPdk>()
                {
                    pdk_editor.set_value(builtin.toml_content());
                }
                let validation = validate_pdk_toml(&pdk_editor.value());
                if validation.valid {
                    let _ = pdk_validation.set_attribute(
                        "style",
                        "padding:4px 8px;border-radius:6px;background:var(--ok-bg);color:var(--ok-fg);border:1px solid var(--ok-border);",
                    );
                    pdk_validation.set_text_content(Some("Valid custom PDK."));
                } else {
                    let _ = pdk_validation.set_attribute(
                        "style",
                        "padding:4px 8px;border-radius:6px;background:var(--err-bg);color:var(--err-fg);border:1px solid var(--err-border);",
                    );
                    pdk_validation.set_text_content(Some(
                        validation.error.as_deref().unwrap_or("Invalid custom PDK."),
                    ));
                }
            } else {
                pdk_select.set_disabled(false);
                template_select.set_value(&pdk_select.value());
                let _ = pdk_editor_wrap.set_attribute("style", "display:none;margin-top:10px;padding:10px;background:var(--gh-subtle);border:1px solid var(--gh-border);border-radius:8px;");
            }
        });
        custom_pdk_toggle
            .add_event_listener_with_callback("change", ontoggle.as_ref().unchecked_ref())?;
        ontoggle.forget();
    }

    {
        let pdk_select = pdk_select.clone();
        let pdk_select_for_handler = pdk_select.clone();
        let template_select = template_select.clone();
        let onchange = Closure::<dyn FnMut(Event)>::new(move |_| {
            template_select.set_value(&pdk_select_for_handler.value());
        });
        pdk_select.add_event_listener_with_callback("change", onchange.as_ref().unchecked_ref())?;
        onchange.forget();
    }

    {
        let template_select = template_select.clone();
        let pdk_editor = pdk_editor.clone();
        let pdk_validation = pdk_validation.clone();
        let onclick = Closure::<dyn FnMut(Event)>::new(move |_| {
            if let Ok(builtin) = template_select.value().parse::<BuiltinPdk>() {
                pdk_editor.set_value(builtin.toml_content());
                if let Ok(Some(storage)) = window().and_then(|w| w.local_storage()) {
                    let _ = storage.set_item(CUSTOM_PDK_STORAGE_KEY, &pdk_editor.value());
                }
                let validation = validate_pdk_toml(&pdk_editor.value());
                if validation.valid {
                    let _ = pdk_validation.set_attribute(
                        "style",
                        "padding:4px 8px;border-radius:6px;background:var(--ok-bg);color:var(--ok-fg);border:1px solid var(--ok-border);",
                    );
                    pdk_validation.set_text_content(Some("Valid custom PDK."));
                } else {
                    let _ = pdk_validation.set_attribute(
                        "style",
                        "padding:4px 8px;border-radius:6px;background:var(--err-bg);color:var(--err-fg);border:1px solid var(--err-border);",
                    );
                    pdk_validation.set_text_content(Some(
                        validation.error.as_deref().unwrap_or("Invalid custom PDK."),
                    ));
                }
            }
        });
        load_template_btn.set_onclick(Some(onclick.as_ref().unchecked_ref()));
        onclick.forget();
    }

    {
        let upload_toml_input = upload_toml_input.clone();
        let onclick = Closure::<dyn FnMut(Event)>::new(move |_| {
            upload_toml_input.click();
        });
        upload_toml_btn.set_onclick(Some(onclick.as_ref().unchecked_ref()));
        onclick.forget();
    }

    {
        let upload_toml_input = upload_toml_input.clone();
        let upload_toml_input_for_handler = upload_toml_input.clone();
        let pdk_editor = pdk_editor.clone();
        let pdk_validation = pdk_validation.clone();
        let onchange = Closure::<dyn FnMut(Event)>::new(move |_| {
            let Some(files) = upload_toml_input_for_handler.files() else {
                return;
            };
            let Some(file) = files.item(0) else {
                return;
            };
            let Ok(reader) = FileReader::new() else {
                return;
            };
            let reader_for_cb = reader.clone();
            let pdk_editor = pdk_editor.clone();
            let pdk_validation = pdk_validation.clone();
            let onload = Closure::<dyn FnMut(Event)>::new(move |_| {
                let Ok(result) = reader_for_cb.result() else {
                    return;
                };
                let Some(text) = result.as_string() else {
                    return;
                };
                pdk_editor.set_value(&text);
                if let Ok(Some(storage)) = window().and_then(|w| w.local_storage()) {
                    let _ = storage.set_item(CUSTOM_PDK_STORAGE_KEY, &text);
                }
                let validation = validate_pdk_toml(&text);
                if validation.valid {
                    let _ = pdk_validation.set_attribute(
                        "style",
                        "padding:4px 8px;border-radius:6px;background:var(--ok-bg);color:var(--ok-fg);border:1px solid var(--ok-border);",
                    );
                    pdk_validation.set_text_content(Some("Valid custom PDK."));
                } else {
                    let _ = pdk_validation.set_attribute(
                        "style",
                        "padding:4px 8px;border-radius:6px;background:var(--err-bg);color:var(--err-fg);border:1px solid var(--err-border);",
                    );
                    pdk_validation.set_text_content(Some(
                        validation.error.as_deref().unwrap_or("Invalid custom PDK."),
                    ));
                }
            });
            reader.set_onload(Some(onload.as_ref().unchecked_ref()));
            onload.forget();
            let _ = reader.read_as_text(&file);
        });
        upload_toml_input
            .add_event_listener_with_callback("change", onchange.as_ref().unchecked_ref())?;
        onchange.forget();
    }

    {
        let pdk_editor = pdk_editor.clone();
        let pdk_editor_for_handler = pdk_editor.clone();
        let pdk_validation = pdk_validation.clone();
        let oninput = Closure::<dyn FnMut(Event)>::new(move |_| {
            let text = pdk_editor_for_handler.value();
            if let Ok(Some(storage)) = window().and_then(|w| w.local_storage()) {
                let _ = storage.set_item(CUSTOM_PDK_STORAGE_KEY, &text);
            }
            let validation = validate_pdk_toml(&text);
            if validation.valid {
                let _ = pdk_validation.set_attribute(
                    "style",
                    "padding:4px 8px;border-radius:6px;background:var(--ok-bg);color:var(--ok-fg);border:1px solid var(--ok-border);",
                );
                pdk_validation.set_text_content(Some("Valid custom PDK."));
            } else {
                let _ = pdk_validation.set_attribute(
                    "style",
                    "padding:4px 8px;border-radius:6px;background:var(--err-bg);color:var(--err-fg);border:1px solid var(--err-border);",
                );
                pdk_validation.set_text_content(Some(
                    validation.error.as_deref().unwrap_or("Invalid custom PDK."),
                ));
            }
        });
        pdk_editor.add_event_listener_with_callback("input", oninput.as_ref().unchecked_ref())?;
        oninput.forget();
    }

    let generate_action: Rc<dyn Fn(bool)> = {
        let rgba_state = Rc::clone(&rgba_state);
        let last_req = Rc::clone(&last_req);
        let last_rects = Rc::clone(&last_rects);
        let pdk_select = pdk_select.clone();
        let custom_pdk_toggle = custom_pdk_toggle.clone();
        let pdk_editor = pdk_editor.clone();
        let strategy_select = strategy_select.clone();
        let invert_toggle = invert_toggle.clone();
        let dither_toggle = dither_toggle.clone();
        let text_input = text_input.clone();
        let text_pos_select = text_pos_select.clone();
        let text_scale = text_scale.clone();
        let qr_input = qr_input.clone();
        let qr_pos_select = qr_pos_select.clone();
        let qr_module_size = qr_module_size.clone();
        let qr_ec_select = qr_ec_select.clone();
        let overlay_margin = overlay_margin.clone();
        let status_el = status.clone();
        let stats_el = stats.clone();
        let drc_badge_el = drc_badge.clone();
        let drc_details_el = drc_details.clone();
        let ctx = ctx.clone();
        let canvas = canvas.clone();
        let download_btn = download_btn.clone();
        let view_state = Rc::clone(&view_state);
        Rc::new(move |fast_preview: bool| {
            let Some((pixels, width, height)) = rgba_state.borrow().as_ref().cloned() else {
                set_status(&status_el, "Load an image first.");
                return;
            };
            let (pixels, width, height) = if fast_preview {
                downsample_rgba_nearest(&pixels, width, height, AUTO_PREVIEW_MAX_DIM)
            } else {
                (pixels, width, height)
            };
            let req = GenerateRequest {
                pixels,
                width,
                height,
                pdk_name: Some(pdk_select.value()),
                custom_pdk_toml: if custom_pdk_toggle.checked() {
                    Some(pdk_editor.value())
                } else {
                    None
                },
                strategy: Some(strategy_select.value()),
                separated: Some(false),
                threshold: Some("128".to_string()),
                invert: Some(invert_toggle.checked()),
                dither: Some(dither_toggle.checked()),
                rotate: Some(0),
                flip: None,
                no_check_drc: Some(fast_preview),
                no_density_enforce: Some(false),
                force: Some(false),
                text: optional_text(text_input.value()),
                text_position: Some(text_pos_select.value()),
                text_scale: Some(parse_u32_input(&text_scale, 1, 1, 32)),
                qr: optional_text(qr_input.value()),
                qr_position: Some(qr_pos_select.value()),
                qr_module_size: Some(parse_u32_input(&qr_module_size, 2, 1, 32)),
                qr_ec_level: Some(qr_ec_select.value()),
                overlay_margin: Some(parse_u32_input(&overlay_margin, 2, 0, 256)),
                cell_name: Some("artwork".to_string()),
                library_name: Some("fabbula".to_string()),
            };
            if custom_pdk_toggle.checked() {
                let validation = validate_pdk_toml(req.custom_pdk_toml.as_deref().unwrap_or(""));
                if !validation.valid {
                    set_status(
                        &status_el,
                        validation
                            .error
                            .as_deref()
                            .unwrap_or("Invalid custom PDK TOML."),
                    );
                    return;
                }
            }

            match run_pipeline(&req) {
                Ok(out) => {
                    *last_rects.borrow_mut() = out.rects.clone();
                    draw_preview(
                        &ctx,
                        canvas.width() as f64,
                        canvas.height() as f64,
                        &last_rects.borrow(),
                        &view_state.borrow(),
                    );

                    let active_pdk = if custom_pdk_toggle.checked() {
                        "custom".to_string()
                    } else {
                        pdk_select.value()
                    };
                    stats_el.set_text_content(Some(&format!(
                        "PDK: {} | Polygons: {} | Bounds: {:.1} um x {:.1} um | Density: {:.1}%",
                        active_pdk,
                        out.stats.polygon_count,
                        out.stats.width_um,
                        out.stats.height_um,
                        out.stats.bitmap_density * 100.0
                    )));

                    if fast_preview {
                        let _ = drc_badge_el.set_attribute(
                            "style",
                            "margin-top:8px;padding:8px;border-radius:8px;background:var(--warn-bg);color:var(--warn-fg);border:1px solid var(--warn-border);",
                        );
                        drc_badge_el.set_text_content(Some("DRC skipped in preview mode"));
                        drc_details_el.set_text_content(Some(
                            "Auto-update uses downsampled preview for speed. Click Generate for full-resolution DRC.",
                        ));
                    } else if out.violations.is_empty() {
                        drc_badge_el.set_attribute(
                            "style",
                            "margin-top:8px;padding:8px;border-radius:8px;background:var(--ok-bg);color:var(--ok-fg);border:1px solid var(--ok-border);",
                        ).ok();
                        drc_badge_el.set_text_content(Some("DRC clean"));
                        drc_details_el.set_text_content(Some("No violations."));
                    } else {
                        drc_badge_el.set_attribute(
                            "style",
                            "margin-top:8px;padding:8px;border-radius:8px;background:var(--err-bg);color:var(--err-fg);border:1px solid var(--err-border);",
                        ).ok();
                        drc_badge_el.set_text_content(Some(&format!(
                            "{} DRC violations",
                            out.violations.len()
                        )));

                        let mut lines = String::new();
                        for v in out.violations.iter().take(50) {
                            let _ = std::fmt::write(
                                &mut lines,
                                format_args!(
                                    "rule={} rect={} other={} value={} limit={} at=({}, {})\n",
                                    v.rule,
                                    v.rect_index,
                                    v.other_index,
                                    v.value,
                                    v.limit,
                                    v.location.x.0,
                                    v.location.y.0
                                ),
                            );
                        }
                        if out.violations.len() > 50 {
                            let _ = std::fmt::write(
                                &mut lines,
                                format_args!("... and {} more\n", out.violations.len() - 50),
                            );
                        }
                        drc_details_el.set_text_content(Some(&lines));
                    }

                    if !fast_preview {
                        *last_req.borrow_mut() = Some(req);
                    }
                    download_btn.set_disabled(false);
                    if fast_preview {
                        set_status(
                            &status_el,
                            &format!(
                                "Preview updated (fast mode, PDK={}). Click Generate for full quality.",
                                if custom_pdk_toggle.checked() {
                                    "custom".to_string()
                                } else {
                                    pdk_select.value()
                                }
                            ),
                        );
                    } else {
                        set_status(
                            &status_el,
                            &format!(
                                "Generation complete (PDK={}).",
                                if custom_pdk_toggle.checked() {
                                    "custom".to_string()
                                } else {
                                    pdk_select.value()
                                }
                            ),
                        );
                    }
                }
                Err(e) => set_status(&status_el, &format!("Generation failed: {}", e)),
            }
        })
    };

    {
        let generate_action = Rc::clone(&generate_action);
        let onclick = Closure::<dyn FnMut(Event)>::new(move |_| {
            generate_action(false);
        });
        generate_btn.set_onclick(Some(onclick.as_ref().unchecked_ref()));
        onclick.forget();
    }

    {
        let generate_action = Rc::clone(&generate_action);
        let download_btn = download_btn.clone();
        let last_req = Rc::clone(&last_req);
        let status_el = status.clone();
        let onkeydown = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
            let is_mod = e.ctrl_key() || e.meta_key();
            if !is_mod {
                return;
            }
            let key = e.key().to_ascii_lowercase();
            if key == "s" {
                e.prevent_default();
                if last_req.borrow().is_none() {
                    set_status(&status_el, "Generate first, then download with Ctrl/Cmd+S.");
                    return;
                }
                download_btn.click();
            } else if key == "enter" {
                e.prevent_default();
                generate_action(false);
            }
        });
        doc.add_event_listener_with_callback("keydown", onkeydown.as_ref().unchecked_ref())?;
        onkeydown.forget();
    }

    if let Some(demo) = load_demo_from_url {
        let kind = if demo.eq_ignore_ascii_case("true") {
            "gradient".to_string()
        } else {
            demo.to_ascii_lowercase()
        };
        let (w, h) = (640u32, 384u32);
        let pixels = demo_rgba(&kind, w, h);
        *rgba_state.borrow_mut() = Some((pixels, w, h));
        set_status(
            &status,
            &format!("Loaded demo image '{}' ({}x{}). Generating...", kind, w, h),
        );
        generate_action(false);
    }

    {
        let window = window()?;
        let debounce_timer = Rc::clone(&debounce_timer);
        let generate_action = Rc::clone(&generate_action);
        let schedule_generate: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(id) = debounce_timer.borrow_mut().take() {
                window.clear_timeout_with_handle(id);
            }
            let run = Rc::clone(&generate_action);
            let cb = Closure::<dyn FnMut()>::new(move || run(true));
            if let Ok(id) = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                200,
            ) {
                *debounce_timer.borrow_mut() = Some(id);
                cb.forget();
            }
        });

        let pdk_select_for_cb = pdk_select.clone();
        let strategy_select_for_cb = strategy_select.clone();
        let invert_for_cb = invert_toggle.clone();
        let dither_for_cb = dither_toggle.clone();
        let custom_for_cb = custom_pdk_toggle.clone();
        let editor_for_cb = pdk_editor.clone();
        let text_for_cb = text_input.clone();
        let text_pos_for_cb = text_pos_select.clone();
        let text_scale_for_cb = text_scale.clone();
        let qr_for_cb = qr_input.clone();
        let qr_pos_for_cb = qr_pos_select.clone();
        let qr_module_for_cb = qr_module_size.clone();
        let qr_ec_for_cb = qr_ec_select.clone();
        let margin_for_cb = overlay_margin.clone();

        let on_pdk = Closure::<dyn FnMut(Event)>::new({
            let schedule_generate = Rc::clone(&schedule_generate);
            move |_| schedule_generate()
        });
        pdk_select_for_cb
            .add_event_listener_with_callback("change", on_pdk.as_ref().unchecked_ref())?;
        on_pdk.forget();

        let on_strategy = Closure::<dyn FnMut(Event)>::new({
            let schedule_generate = Rc::clone(&schedule_generate);
            move |_| schedule_generate()
        });
        strategy_select_for_cb
            .add_event_listener_with_callback("change", on_strategy.as_ref().unchecked_ref())?;
        on_strategy.forget();

        let on_invert = Closure::<dyn FnMut(Event)>::new({
            let schedule_generate = Rc::clone(&schedule_generate);
            move |_| schedule_generate()
        });
        invert_for_cb
            .add_event_listener_with_callback("change", on_invert.as_ref().unchecked_ref())?;
        on_invert.forget();

        let on_dither = Closure::<dyn FnMut(Event)>::new({
            let schedule_generate = Rc::clone(&schedule_generate);
            move |_| schedule_generate()
        });
        dither_for_cb
            .add_event_listener_with_callback("change", on_dither.as_ref().unchecked_ref())?;
        on_dither.forget();

        let on_custom_toggle = Closure::<dyn FnMut(Event)>::new({
            let schedule_generate = Rc::clone(&schedule_generate);
            move |_| schedule_generate()
        });
        custom_for_cb.add_event_listener_with_callback(
            "change",
            on_custom_toggle.as_ref().unchecked_ref(),
        )?;
        on_custom_toggle.forget();

        let on_editor_input = Closure::<dyn FnMut(Event)>::new({
            let schedule_generate = Rc::clone(&schedule_generate);
            move |_| schedule_generate()
        });
        editor_for_cb
            .add_event_listener_with_callback("input", on_editor_input.as_ref().unchecked_ref())?;
        on_editor_input.forget();

        let on_text_input = Closure::<dyn FnMut(Event)>::new({
            let schedule_generate = Rc::clone(&schedule_generate);
            move |_| schedule_generate()
        });
        text_for_cb
            .add_event_listener_with_callback("input", on_text_input.as_ref().unchecked_ref())?;
        on_text_input.forget();

        let on_text_pos = Closure::<dyn FnMut(Event)>::new({
            let schedule_generate = Rc::clone(&schedule_generate);
            move |_| schedule_generate()
        });
        text_pos_for_cb
            .add_event_listener_with_callback("change", on_text_pos.as_ref().unchecked_ref())?;
        on_text_pos.forget();

        let on_text_scale = Closure::<dyn FnMut(Event)>::new({
            let schedule_generate = Rc::clone(&schedule_generate);
            move |_| schedule_generate()
        });
        text_scale_for_cb
            .add_event_listener_with_callback("input", on_text_scale.as_ref().unchecked_ref())?;
        on_text_scale.forget();

        let on_qr_input = Closure::<dyn FnMut(Event)>::new({
            let schedule_generate = Rc::clone(&schedule_generate);
            move |_| schedule_generate()
        });
        qr_for_cb
            .add_event_listener_with_callback("input", on_qr_input.as_ref().unchecked_ref())?;
        on_qr_input.forget();

        let on_qr_pos = Closure::<dyn FnMut(Event)>::new({
            let schedule_generate = Rc::clone(&schedule_generate);
            move |_| schedule_generate()
        });
        qr_pos_for_cb
            .add_event_listener_with_callback("change", on_qr_pos.as_ref().unchecked_ref())?;
        on_qr_pos.forget();

        let on_qr_module = Closure::<dyn FnMut(Event)>::new({
            let schedule_generate = Rc::clone(&schedule_generate);
            move |_| schedule_generate()
        });
        qr_module_for_cb
            .add_event_listener_with_callback("input", on_qr_module.as_ref().unchecked_ref())?;
        on_qr_module.forget();

        let on_qr_ec = Closure::<dyn FnMut(Event)>::new({
            let schedule_generate = Rc::clone(&schedule_generate);
            move |_| schedule_generate()
        });
        qr_ec_for_cb
            .add_event_listener_with_callback("change", on_qr_ec.as_ref().unchecked_ref())?;
        on_qr_ec.forget();

        let on_margin = Closure::<dyn FnMut(Event)>::new({
            let schedule_generate = Rc::clone(&schedule_generate);
            move |_| schedule_generate()
        });
        margin_for_cb
            .add_event_listener_with_callback("input", on_margin.as_ref().unchecked_ref())?;
        on_margin.forget();
    }

    {
        let last_rects = Rc::clone(&last_rects);
        let view_state = Rc::clone(&view_state);
        let ctx = ctx.clone();
        let canvas = canvas.clone();
        let canvas_for_draw = canvas.clone();

        let onwheel = Closure::<dyn FnMut(WheelEvent)>::new(move |e: WheelEvent| {
            e.prevent_default();
            let mut view = view_state.borrow_mut();
            let old_zoom = view.zoom;
            if e.delta_y() < 0.0 {
                view.zoom = (view.zoom * 1.1).min(20.0);
            } else {
                view.zoom = (view.zoom / 1.1).max(0.1);
            }

            let rects = last_rects.borrow();
            if let (Some((old_scale, old_ox, old_oy)), Some((new_scale, new_ox, new_oy))) = (
                preview_transform(
                    canvas_for_draw.width() as f64,
                    canvas_for_draw.height() as f64,
                    &rects,
                    old_zoom,
                ),
                preview_transform(
                    canvas_for_draw.width() as f64,
                    canvas_for_draw.height() as f64,
                    &rects,
                    view.zoom,
                ),
            ) {
                let sx = e.offset_x() as f64;
                let sy = e.offset_y() as f64;
                let world_x = (sx - view.pan_x - old_ox) / old_scale;
                let world_y =
                    (canvas_for_draw.height() as f64 - sy + view.pan_y - old_oy) / old_scale;
                view.pan_x = sx - new_ox - world_x * new_scale;
                view.pan_y =
                    sy - (canvas_for_draw.height() as f64 - (new_oy + world_y * new_scale));
            }

            draw_preview(
                &ctx,
                canvas_for_draw.width() as f64,
                canvas_for_draw.height() as f64,
                &rects,
                &view,
            );
        });
        canvas.add_event_listener_with_callback("wheel", onwheel.as_ref().unchecked_ref())?;
        onwheel.forget();
    }

    {
        let view_state = Rc::clone(&view_state);
        let canvas = canvas.clone();
        let onmousedown = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
            let mut view = view_state.borrow_mut();
            view.dragging = true;
            view.last_x = e.offset_x() as f64;
            view.last_y = e.offset_y() as f64;
        });
        canvas
            .add_event_listener_with_callback("mousedown", onmousedown.as_ref().unchecked_ref())?;
        onmousedown.forget();
    }

    {
        let last_rects = Rc::clone(&last_rects);
        let view_state = Rc::clone(&view_state);
        let ctx = ctx.clone();
        let canvas = canvas.clone();
        let canvas_for_draw = canvas.clone();
        let onmousemove = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
            let mut view = view_state.borrow_mut();
            if !view.dragging {
                return;
            }
            let x = e.offset_x() as f64;
            let y = e.offset_y() as f64;
            view.pan_x += x - view.last_x;
            view.pan_y += y - view.last_y;
            view.last_x = x;
            view.last_y = y;
            draw_preview(
                &ctx,
                canvas_for_draw.width() as f64,
                canvas_for_draw.height() as f64,
                &last_rects.borrow(),
                &view,
            );
        });
        canvas
            .add_event_listener_with_callback("mousemove", onmousemove.as_ref().unchecked_ref())?;
        onmousemove.forget();
    }

    {
        let view_state = Rc::clone(&view_state);
        let canvas = canvas.clone();
        let onup = Closure::<dyn FnMut(Event)>::new(move |_| {
            let mut view = view_state.borrow_mut();
            view.dragging = false;
        });
        canvas.add_event_listener_with_callback("mouseup", onup.as_ref().unchecked_ref())?;
        canvas.add_event_listener_with_callback("mouseleave", onup.as_ref().unchecked_ref())?;
        onup.forget();
    }

    {
        let last_req = Rc::clone(&last_req);
        let status_el = status.clone();
        let doc = doc.clone();
        let onclick = Closure::<dyn FnMut(Event)>::new(move |_| {
            let Some(req) = last_req.borrow().as_ref().cloned() else {
                set_status(&status_el, "Generate first.");
                return;
            };
            let Ok(bytes) = generate_gds_bytes(req) else {
                set_status(&status_el, "Failed to serialize GDS.");
                return;
            };
            let array = js_sys::Uint8Array::from(bytes.as_slice());
            let parts = js_sys::Array::new();
            parts.push(&array);
            let Ok(blob) = Blob::new_with_u8_array_sequence(&parts) else {
                set_status(&status_el, "Failed to create download blob.");
                return;
            };
            let Ok(url) = Url::create_object_url_with_blob(&blob) else {
                set_status(&status_el, "Failed to create download URL.");
                return;
            };
            let Ok(anchor): Result<HtmlAnchorElement, _> = doc
                .create_element("a")
                .and_then(|e| e.dyn_into::<HtmlAnchorElement>().map_err(Into::into))
            else {
                set_status(&status_el, "Failed to create anchor.");
                let _ = Url::revoke_object_url(&url);
                return;
            };
            anchor.set_href(&url);
            anchor.set_download("artwork.gds");
            anchor.click();
            let _ = Url::revoke_object_url(&url);
        });
        download_btn.set_onclick(Some(onclick.as_ref().unchecked_ref()));
        onclick.forget();
    }

    Ok(())
}
