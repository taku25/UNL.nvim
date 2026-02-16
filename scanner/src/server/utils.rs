use serde_json::Value;

pub fn convert_params<T: serde::de::DeserializeOwned>(val: &Value) -> anyhow::Result<T> {
    Ok(serde_json::from_value(val.clone())?)
}

pub fn normalize_to_unix(s: &str) -> String {
    s.replace('\\', "/")
}

pub fn normalize_to_native(s: &str) -> String {
    if cfg!(target_os = "windows") {
        s.replace('/', "\\")
    } else {
        s.replace('\\', "/")
    }
}

pub fn normalize_path_key(s: &str) -> String {
    let mut normalized = s.replace('\\', "/");
    while normalized.ends_with('/') && normalized.len() > 3 {
        normalized.pop();
    }
    normalized
}
