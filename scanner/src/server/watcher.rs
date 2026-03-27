use std::sync::{Arc};
use std::path::{PathBuf};
use crate::server::state::{AppState};
use crate::server::utils::{normalize_to_native};
use crate::{scanner, db};

pub async fn handle_file_change(state: Arc<AppState>, path: PathBuf) {
    let path_str_native = path.to_string_lossy().to_string();
    
    // Windows UNC path support: remove "\\?\" or "//?/" prefix if it exists
    let path_str_clean = if path_str_native.starts_with(r"\\?\") {
        &path_str_native[4..]
    } else if path_str_native.starts_with("//?/") {
        &path_str_native[4..]
    } else {
        &path_str_native
    };

    let mut path_str_unix = path_str_clean.replace('\\', "/");
    
    // Windows: Normalize drive letter (c:/ -> C:/)
    if cfg!(target_os = "windows") {
        if path_str_unix.len() >= 2 && &path_str_unix[1..2] == ":" {
            let drive = &path_str_unix[0..1].to_uppercase();
            path_str_unix.replace_range(0..1, drive);
        }
    }

    tracing::debug!("Watcher: processing event for: {}", path_str_unix);

    if !path.exists() { 
        tracing::debug!("Watcher: path does not exist, skipping: {}", path_str_unix);
        return; 
    }
    
    let path_str_unix_lower = path_str_unix.to_lowercase();

    let target = {
        let projects = state.projects.lock().unwrap();
        let mut res = None;
        for (root, ctx) in projects.iter() {
            let root_lower = root.to_lowercase();
            if path_str_unix_lower.starts_with(&root_lower) {
                res = Some((root.clone(), ctx.db_path.clone()));
                break;
            }
        }
        res
    };

    if let Some((root_clone, db_path_unix)) = target {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        
        if ext == "ini" {
            let mut caches = state.config_caches.lock().unwrap();
            if let Some(cache) = caches.get_mut(&root_clone) {
                cache.is_dirty = true;
                tracing::info!("Config cache marked as dirty: {}", path_str_unix);
            }
            return;
        }

        if ext == "uasset" || ext == "umap" {
            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if ext == "umap" || filename.starts_with("BP_") || filename.starts_with("ABP_") || filename.starts_with("WBP_") || filename.starts_with("AM_") || filename.starts_with("DA_") || filename.starts_with("DT_") {
                crate::server::asset::update_single_asset(state.clone(), &root_clone, &path).await;
            }
            return;
        }

        if !["h", "cpp", "hpp", "cs"].contains(&ext.as_str()) { 
            tracing::debug!("Watcher: ignoring file with extension: {}", ext);
            return; 
        }
        
        let db_path_native = normalize_to_native(&db_path_unix);
        let conn_arc = match state.get_connection(&db_path_native) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Watcher: Failed to get DB connection for {}: {}", path_str_unix, e);
                return;
            }
        };
        
        let path_str_for_scan = path_str_unix.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = conn_arc.lock().unwrap();
            match db::get_module_id_for_path(&conn, &path_str_for_scan) {
                Ok(Some(mod_id)) => {
                    tracing::info!("File change detected, re-scanning: {}", path_str_for_scan);
                    let language = tree_sitter_unreal_cpp::LANGUAGE.into();
                    let query = tree_sitter::Query::new(&language, scanner::QUERY_STR).unwrap();
                    let mtime = std::fs::metadata(&path_str_for_scan).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
                    let input = crate::types::InputFile { 
                        path: path_str_for_scan, 
                        mtime, 
                        old_hash: None, 
                        module_id: Some(mod_id), 
                        db_path: Some(db_path_native.clone()) 
                    };
                    if let Ok(res) = scanner::process_file(&input, &language, &query) { 
                        let classes_to_invalidate = if let Some(data) = &res.data {
                            data.classes.iter().map(|c| c.class_name.clone()).collect::<Vec<String>>()
                        } else {
                            Vec::new()
                        };

                        if let Err(e) = db::save_to_db(&mut conn, &[res], Arc::new(crate::types::StdoutReporter)) {
                            tracing::error!("Watcher: Failed to save scan results to DB: {}", e);
                        } else {
                            // Invalidate cache for classes found in this file
                            let cache_arc = state.get_completion_cache(&root_clone);
                            let mut cache = cache_arc.lock().unwrap();
                            for cls in classes_to_invalidate {
                                cache.invalidate_class(&cls);
                            }
                        }
                    }
                }
                Ok(None) => {
                    tracing::warn!("Watcher: Could not identify module for file: {}. Skipping scan.", path_str_for_scan);
                }
                Err(e) => {
                    tracing::error!("Watcher: DB error looking up module for {}: {}", path_str_for_scan, e);
                }
            }
        });
    } else {
        tracing::debug!("Watcher: path is outside of any project root: {}", path_str_unix_lower);
    }
}
