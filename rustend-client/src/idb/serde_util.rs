use serde::Serialize;
use crate::error::RustendClientError;

pub fn to_js<T: Serialize>(value: &T) -> Result<wasm_bindgen::JsValue, RustendClientError> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))
}

pub fn from_js<T: serde::de::DeserializeOwned>(
    val: wasm_bindgen::JsValue,
) -> Result<T, RustendClientError> {
    let json: serde_json::Value = serde_wasm_bindgen::from_value(val)
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
    serde_json::from_value(json)
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))
}
