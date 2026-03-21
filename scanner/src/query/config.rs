use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use serde_json::Value;
use anyhow::Result;
use std::fs;
use crate::types::{ConfigPlatform, ConfigSection, ConfigParameter, ConfigHistory, ConfigCache};
use crate::server::state::AppState;

struct IniItem {
    key: String,
    value: String,
    op: String,
    line: usize,
}

struct IniParsed {
    sections: HashMap<String, Vec<IniItem>>,
}

fn parse_ini(path: &Path) -> Result<IniParsed> {
    let content = fs::read_to_string(path)?;
    let mut sections = HashMap::new();
    let mut current_section = "Default".to_string();
    
    for (idx, line) in content.lines().enumerate() {
        let line_num = idx + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
            continue;
        }
        
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            current_section = trimmed[1..trimmed.len()-1].to_string();
            continue;
        }
        
        if let Some(pos) = trimmed.find('=') {
            let mut key_part = trimmed[..pos].trim();
            let value = trimmed[pos+1..].trim().to_string();
            
            let op = if key_part.starts_with('+') {
                key_part = &key_part[1..];
                "+"
            } else if key_part.starts_with('-') {
                key_part = &key_part[1..];
                "-"
            } else if key_part.starts_with('!') {
                key_part = &key_part[1..];
                "!"
            } else {
                ""
            };
            
            sections.entry(current_section.clone())
                .or_insert_with(Vec::new)
                .push(IniItem {
                    key: key_part.to_string(),
                    value,
                    op: op.to_string(),
                    line: line_num,
                });
        }
    }
    
    Ok(IniParsed { sections })
}

fn apply_op(current: Option<Value>, op: &str, new_value: &str) -> Value {
    use serde_json::json;
    
    if op == "!" {
        return Value::Null;
    }
    
    let new_val_json = Value::String(new_value.to_string());
    
    match op {
        "-" => {
            if let Some(Value::Array(mut arr)) = current {
                arr.retain(|v| v != &new_val_json);
                if arr.is_empty() { Value::Null } else { Value::Array(arr) }
            } else if current == Some(new_val_json) {
                Value::Null
            } else {
                current.unwrap_or(Value::Null)
            }
        },
        "+" => {
            match current {
                Some(Value::Array(mut arr)) => {
                    arr.push(new_val_json);
                    Value::Array(arr)
                },
                Some(v) => {
                    if v.is_null() {
                        json!([new_value])
                    } else {
                        json!([v, new_value])
                    }
                },
                None => json!([new_value])
            }
        },
        _ => new_val_json
    }
}

fn format_value(val: &Value) -> String {
    match val {
        Value::Array(arr) => {
            if let Some(last) = arr.last() {
                format!("[Array x{}] {}", arr.len(), last.as_str().unwrap_or(""))
            } else {
                "[]".to_string()
            }
        },
        Value::String(s) => {
            if s.len() > 50 {
                format!("{}...", &s[..47])
            } else {
                s.clone()
            }
        },
        Value::Null => "nil".to_string(),
        _ => val.to_string(),
    }
}

struct ConfigSource {
    path: PathBuf,
    name: String,
}

pub fn get_config_data_with_cache(state: &AppState, project_root_str: &str, engine_root_opt: Option<&str>) -> Result<Vec<ConfigPlatform>> {
    let root_key = crate::server::utils::normalize_path_key(project_root_str);
    
    {
        let caches = state.config_caches.lock().unwrap();
        if let Some(cache) = caches.get(&root_key) {
            if !cache.is_dirty {
                return Ok(cache.data.clone());
            }
        }
    }

    // Rebuild
    let data = get_config_data_internal(project_root_str, engine_root_opt)?;
    
    {
        let mut caches = state.config_caches.lock().unwrap();
        caches.insert(root_key, ConfigCache {
            data: data.clone(),
            is_dirty: false,
        });
    }
    
    Ok(data)
}

fn get_config_data_internal(project_root_str: &str, engine_root_opt: Option<&str>) -> Result<Vec<ConfigPlatform>> {
    let project_root = PathBuf::from(project_root_str);
    let engine_root = engine_root_opt.map(PathBuf::from);
    
    let platforms = get_available_platforms(&project_root, engine_root.as_deref());
    let mut results = Vec::new();
    
    // Default (Editor)
    results.push(resolve_platform("Default (Editor)", "Default", false, &project_root, engine_root.as_deref())?);
    
    // Other platforms
    for p in platforms {
        results.push(resolve_platform(&p, &p, false, &project_root, engine_root.as_deref())?);
    }
    
    Ok(results)
}

fn get_available_platforms(project_root: &Path, engine_root: Option<&Path>) -> Vec<String> {
    let mut platforms = std::collections::BTreeSet::new();
    
    let mut check_dirs = Vec::new();
    if let Some(er) = engine_root {
        check_dirs.push(er.join("Engine/Config"));
        check_dirs.push(er.join("Engine/Platforms"));
    }
    check_dirs.push(project_root.join("Config"));
    check_dirs.push(project_root.join("Platforms"));
    
    for dir in check_dirs {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Ok(file_type) = entry.file_type() {
                    if file_type.is_dir() || file_type.is_symlink() {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        if name.starts_with('.') { continue; }
                        
                        let path = entry.path();
                        let check_paths = [
                            path.join(format!("{}Engine.ini", name)),
                            path.join("DataDrivenPlatformInfo.ini"),
                            path.join("Config"),
                        ];
                        
                        for cp in &check_paths {
                            if cp.exists() {
                                platforms.insert(name.clone());
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    
    let major = ["Windows", "Mac", "Linux", "Android", "IOS", "TVOS", "Apple", "Unix"];
    for &p in &major {
        if platforms.contains(p) { continue; }
        let p_name = p.to_string();
        let paths = [
            engine_root.map(|er| er.join("Engine/Config").join(&p_name)),
            engine_root.map(|er| er.join("Engine/Platforms").join(&p_name)),
            Some(project_root.join("Config").join(&p_name)),
            Some(project_root.join("Platforms").join(&p_name)),
        ];
        for path_opt in &paths {
            if let Some(path) = path_opt {
                if path.is_dir() {
                    platforms.insert(p_name.clone());
                    break;
                }
            }
        }
    }
    
    platforms.into_iter().collect()
}

fn resolve_platform(name: &str, platform: &str, is_profile: bool, project_root: &Path, engine_root: Option<&Path>) -> Result<ConfigPlatform> {
    let stack = get_config_stack(project_root, engine_root, platform);
    
    let mut resolved_sections: BTreeMap<String, BTreeMap<String, (Value, Vec<ConfigHistory>)>> = BTreeMap::new();
    
    for source in stack {
        if let Ok(parsed) = parse_ini(&source.path) {
            let file_name = source.name;
            let full_path = source.path.to_string_lossy().into_owned();
            
            for (section_name, items) in parsed.sections {
                let section = resolved_sections.entry(section_name).or_insert_with(BTreeMap::new);
                for item in items {
                    let entry = section.entry(item.key.clone()).or_insert_with(|| (Value::Null, Vec::new()));
                    entry.0 = apply_op(if entry.0.is_null() { None } else { Some(entry.0.clone()) }, &item.op, &item.value);
                    
                    entry.1.push(ConfigHistory {
                        file: file_name.clone(),
                        full_path: full_path.clone(),
                        value: format_value(&entry.0),
                        op: item.op,
                        line: item.line,
                    });
                }
            }
        }
    }
    
    let mut sections = Vec::new();
    for (s_name, params_map) in resolved_sections {
        let mut parameters = Vec::new();
        for (p_key, (final_val, history)) in params_map {
            parameters.push(ConfigParameter {
                key: p_key,
                value: format_value(&final_val),
                history,
            });
        }
        sections.push(ConfigSection {
            name: s_name,
            parameters,
        });
    }
    
    Ok(ConfigPlatform {
        name: name.to_string(),
        platform: platform.to_string(),
        is_profile,
        sections,
    })
}

fn get_config_stack(project_root: &Path, engine_root: Option<&Path>, platform: &str) -> Vec<ConfigSource> {
    let mut stack = Vec::new();
    
    fn push_if_exists(stack: &mut Vec<ConfigSource>, p: PathBuf) {
        if p.is_file() {
            let mut name = p.file_name().unwrap().to_string_lossy().into_owned();
            let parent = p.parent().unwrap();
            if parent.file_name().map(|n| n != "Config").unwrap_or(false) {
                name = format!("{}/{}", parent.file_name().unwrap().to_string_lossy(), name);
            }
            stack.push(ConfigSource { path: p, name });
        }
    }

    fn push_all_in_dir(stack: &mut Vec<ConfigSource>, dir: PathBuf) {
        if dir.is_dir() {
            if let Ok(entries) = fs::read_dir(dir) {
                let mut files: Vec<_> = entries.flatten().collect();
                files.sort_by_key(|e| e.file_name());
                for entry in files {
                    if entry.path().extension().map(|ext| ext == "ini").unwrap_or(false) {
                        push_if_exists(stack, entry.path());
                    }
                }
            }
        }
    }

    if let Some(er) = engine_root {
        push_if_exists(&mut stack, er.join("Engine/Config/Base.ini"));
        push_if_exists(&mut stack, er.join("Engine/Config/BaseEngine.ini"));
        
        if platform == "Mac" || platform == "IOS" || platform == "TVOS" || platform == "Apple" {
            push_if_exists(&mut stack, er.join("Engine/Config/Apple/AppleEngine.ini"));
        }
        if platform == "Linux" || platform == "Unix" {
            push_if_exists(&mut stack, er.join("Engine/Config/Unix/UnixEngine.ini"));
        }
        
        if platform != "Default" {
            push_if_exists(&mut stack, er.join("Engine/Config").join(platform).join(format!("{}Engine.ini", platform)));
            push_all_in_dir(&mut stack, er.join("Engine/Platforms").join(platform).join("Config"));
        }
    }
    
    push_if_exists(&mut stack, project_root.join("Config/DefaultEngine.ini"));
    push_all_in_dir(&mut stack, project_root.join("Platforms/Config"));
    
    if platform != "Default" {
        push_if_exists(&mut stack, project_root.join("Config").join(platform).join(format!("{}Engine.ini", platform)));
        push_all_in_dir(&mut stack, project_root.join("Platforms").join(platform).join("Config"));
    }
    
    stack
}
