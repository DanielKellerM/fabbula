// Copyright 2026 Daniel Keller <daniel.keller.m@gmail.com>
// Licensed under the Apache License, Version 2.0.
// SPDX-License-Identifier: Apache-2.0

use crate::wasm_app::{self, GenerateRequest};
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::wasm_bindgen;

fn js_err<E: std::fmt::Display>(e: E) -> JsValue {
    JsValue::from_str(&e.to_string())
}

#[wasm_bindgen]
pub fn generate_from_pixels(req: JsValue) -> Result<JsValue, JsValue> {
    let req: GenerateRequest = serde_wasm_bindgen::from_value(req).map_err(js_err)?;
    let response = wasm_app::generate_from_pixels(req).map_err(js_err)?;
    serde_wasm_bindgen::to_value(&response).map_err(js_err)
}

#[wasm_bindgen]
pub fn generate_gds_bytes(req: JsValue) -> Result<Vec<u8>, JsValue> {
    let req: GenerateRequest = serde_wasm_bindgen::from_value(req).map_err(js_err)?;
    wasm_app::generate_gds_bytes(req).map_err(js_err)
}

#[wasm_bindgen]
pub fn validate_pdk_toml(toml_content: &str) -> JsValue {
    let response = wasm_app::validate_pdk_toml(toml_content);
    serde_wasm_bindgen::to_value(&response)
        .unwrap_or_else(|e| JsValue::from_str(&format!("serialization error: {}", e)))
}

#[wasm_bindgen]
pub fn list_builtin_pdks() -> Result<JsValue, JsValue> {
    let out = wasm_app::list_builtin_pdks().map_err(js_err)?;
    serde_wasm_bindgen::to_value(&out).map_err(js_err)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn start_app() -> Result<(), JsValue> {
    crate::wasm_dom::start_app()
}
