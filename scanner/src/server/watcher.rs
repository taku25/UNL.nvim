use std::sync::{Arc};
use std::path::{PathBuf};
use crate::server::state::{AppState};
use crate::server::utils::{normalize_to_native};
use crate::{scanner, db};

pub async fn handle_file_change(state: Arc<AppState>, path: PathBuf) {
    if !path.exists() { return; }
    let target = {
        let projects = state.projects.lock().unwrap();
        let mut res = None;
        for (root, ctx) in projects.iter() {
            if path.to_string_lossy().replace('\\', "/").starts_with(root) {
                res = Some((root.clone(), ctx.db_path.clone()));
                break;
            }
        }
        res
    };
    if let Some((root_clone, db_path_unix)) = target {
        let path_str = path.to_string_lossy().replace("\\", "/");
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        
        if ext == "ini" {
            let mut caches = state.config_caches.lock().unwrap();
            if let Some(cache) = caches.get_mut(&root_clone) {
                cache.is_dirty = true;
                tracing::info!("Config cache marked as dirty due to change in: {}", path_str);
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

        if !["h", "cpp", "hpp", "cs"].contains(&ext) { return; }
        
        let db_path_native = normalize_to_native(&db_path_unix);
        let conn_arc = match state.get_connection(&db_path_native) {
            Ok(c) => c,
            Err(_) => return,
        };
        
        let path_str_clone = path_str.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = conn_arc.lock().unwrap();
            if let Ok(Some(mod_id)) = db::get_module_id_for_path(&conn, &path_str_clone) {
                let language = tree_sitter_unreal_cpp::LANGUAGE.into();
                let query = tree_sitter::Query::new(&language, scanner::QUERY_STR).unwrap();
                let mtime = std::fs::metadata(&path_str_clone).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
                let input = crate::types::InputFile { path: path_str_clone, mtime, old_hash: None, module_id: Some(mod_id), db_path: Some(db_path_native.clone()) };
                if let Ok(res) = scanner::process_file(&input, &language, &query) { 
                    let _ = db::save_to_db(&mut conn, &[res], Arc::new(crate::types::StdoutReporter));
                }
            }
        });
    }
}
