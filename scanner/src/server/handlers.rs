use std::sync::{Arc};
use std::path::{PathBuf};
use std::time::{Instant};
use std::collections::{HashSet};
use tokio::sync::mpsc;
use tracing::{info};
use notify::Watcher;
use serde::Deserialize;
use serde_json::{json, Value};
use crate::server::state::{AppState, ProjectContext, RpcProgressReporter};
use crate::server::utils::{convert_params, normalize_to_unix, normalize_to_native, normalize_path_key};
use crate::server::asset::handle_asset_scan;
use crate::types::{RefreshRequest, ScanRequest, QueryRequest, SetupRequest};
use crate::{scanner, db, refresh};

#[derive(Deserialize)]
pub struct DeleteProjectRequest { pub project_root: String }

pub async fn handle_delete_project(state: &AppState, params: &Value) -> anyhow::Result<Value> {
    let req: DeleteProjectRequest = convert_params(params)?;
    let root_key = normalize_path_key(&req.project_root);
    
    let removed = {
        let mut projects = state.projects.lock().unwrap();
        projects.remove(&root_key).is_some()
    };
    
    if removed {
        let _ = state.save_registry();
        info!("Deleted project: {}", root_key);
        Ok(Value::String("Deleted".to_string()))
    } else {
        Err(anyhow::anyhow!("Project not found"))
    }
}

#[derive(Deserialize)]
pub struct PingRequest { pub pid: u32 }

pub async fn handle_ping(state: &AppState, params: &Value) -> anyhow::Result<Value> {
    let req: PingRequest = convert_params(params)?;
    state.register_client(req.pid);
    Ok(Value::String("pong".to_string()))
}

pub async fn handle_setup(state: Arc<AppState>, params: &Value) -> anyhow::Result<Value> {
    let req: SetupRequest = convert_params(params)?;
    let db_path_native = normalize_to_native(&req.db_path);
    
    {
        let mut conns = state.connections.lock().unwrap();
        conns.remove(&db_path_native);
    }

    let root_key = normalize_path_key(&req.project_root);
    let db_path_native_clone = db_path_native.clone();
    let was_empty = tokio::task::spawn_blocking(move || {
        let mut is_new = false;
        if let Ok(conn) = rusqlite::Connection::open(&db_path_native_clone) {
            // Check if classes table is empty
            if let Ok(count) = conn.query_row("SELECT COUNT(*) FROM classes", [], |r| r.get::<_, i64>(0)) {
                if count == 0 { is_new = true; }
            }
        }
        Ok::<bool, anyhow::Error>(is_new)
    }).await??;

    {
        let mut projects = state.projects.lock().unwrap();
        projects.insert(root_key.clone(), ProjectContext { db_path: normalize_to_unix(&req.db_path), vcs_hash: req.vcs_hash.clone(), _last_refresh: Instant::now() });
    }
    let _ = state.get_connection(&db_path_native);
    let _ = state.save_registry();

    let state_clone = state.clone();
    let root_clone = root_key.clone();
    tokio::spawn(async move {
        handle_asset_scan(state_clone, root_clone).await;
    });

    Ok(serde_json::json!({ "status": "ok", "needs_full_refresh": was_empty }))
}

pub async fn handle_refresh(state: &AppState, params: &Value, tx: mpsc::Sender<Vec<u8>>) -> anyhow::Result<Value> {
    let mut req: RefreshRequest = convert_params(params)?;
    let root_key = normalize_path_key(&req.project_root);

    {
        let mut active = state.active_refreshes.lock().unwrap();
        if active.contains(&root_key) {
            info!("Refresh already in progress for project: {}. Skipping redundant request.", root_key);
            return Ok(Value::String("Refresh already in progress".to_string()));
        }
        active.insert(root_key.clone());
    }

    struct RefreshGuard<'a> {
        state: &'a AppState,
        root: String,
    }
    impl<'a> Drop for RefreshGuard<'a> {
        fn drop(&mut self) {
            let mut active = self.state.active_refreshes.lock().unwrap();
            active.remove(&self.root);
        }
    }
    let _guard = RefreshGuard { state, root: root_key.clone() };

    let db_path_unix = {
        let mut projects = state.projects.lock().unwrap();
        if let Some(path) = &req.db_path {
             let path_u = normalize_to_unix(path);
             projects.insert(root_key.clone(), ProjectContext { db_path: path_u.clone(), vcs_hash: req.vcs_hash.clone(), _last_refresh: Instant::now() });
             path_u
        } else if let Some(ctx) = projects.get_mut(&root_key) {
             ctx.vcs_hash = req.vcs_hash.clone();
             ctx.db_path.clone()
        } else { return Err(anyhow::anyhow!("Project not found")); }
    };
    
    let db_path_native = normalize_to_native(&db_path_unix);
    
    {
        let mut conns = state.connections.lock().unwrap();
        conns.remove(&db_path_native);
    }

    req.db_path = Some(db_path_unix.clone());
    let _ = state.save_registry();
    let reporter = Arc::new(RpcProgressReporter { tx });
    tokio::task::spawn_blocking(move || { refresh::run_refresh(req, reporter) }).await??;
    
    // Clear completion cache after refresh
    {
        let cache_arc = state.get_completion_cache(&root_key);
        let mut cache = cache_arc.lock().unwrap();
        cache.clear();
        info!("Cleared completion cache after refresh for: {}", root_key);
    }

    let db_path_native = normalize_to_native(&db_path_unix);
    let _ = state.get_connection(&db_path_native);
    
    Ok(Value::String("Refresh success".to_string()))
}

pub async fn handle_watch(state: &AppState, params: &Value) -> anyhow::Result<Value> {
    let req: crate::types::WatchRequest = convert_params(params)?;
    let root_native = normalize_to_native(&req.project_root);
    tracing::info!("Watcher: Request received to watch path: {}", root_native);
    
    let root_path_native = PathBuf::from(&root_native);
    if !root_path_native.exists() {
        tracing::error!("Watcher: Path does not exist: {}", root_native);
        return Err(anyhow::anyhow!("Path does not exist: {}", root_native));
    }

    let mut watcher = state.watcher.lock().unwrap();
    match watcher.watch(&root_path_native, notify::RecursiveMode::Recursive) {
        Ok(_) => {
            tracing::info!("Watcher: Successfully started watching: {}", root_native);
            Ok(Value::String("Watch started".to_string()))
        }
        Err(e) => {
            tracing::error!("Watcher: Failed to start watching {}: {}", root_native, e);
            Err(e.into())
        }
    }
}

#[derive(serde::Deserialize)]
pub struct ServerQueryRequest { pub project_root: String, #[serde(flatten)] pub query: QueryRequest }

pub async fn handle_query(state: Arc<AppState>, params: &Value, tx: mpsc::Sender<Vec<u8>>, msgid: u64) -> anyhow::Result<Value> {
    let req: ServerQueryRequest = convert_params(params)?;
    let root_key = normalize_path_key(&req.project_root);
    
    // --- Added: Block during Refresh ---
    {
        let active = state.active_refreshes.lock().unwrap();
        if active.contains(&root_key) {
            // tracing::debug!("Blocking query during active refresh for: {}", root_key);
            return Ok(json!([])); // Refresh中は即座に空を返す
        }
    }
    // ------------------------------------

    {
        let graphs = state.asset_graphs.lock().unwrap();
        if !graphs.contains_key(&root_key) {
            let mut active_scans = state.active_asset_scans.lock().unwrap();
            if !active_scans.contains(&root_key) {
                active_scans.insert(root_key.clone());
                info!("Launching targeted asset scan: {} (Key: {})", req.project_root, root_key);
                let state_clone = state.clone();
                let root_clone = req.project_root.clone();
                tokio::spawn(async move {
                    handle_asset_scan(state_clone, root_clone).await;
                });
            }
        }
    }

    let db_path_unix = {
        let projects = state.projects.lock().unwrap();
        let ctx = projects.get(&root_key).ok_or_else(|| anyhow::anyhow!("Project not found"))?;
        ctx.db_path.clone()
    };
    let db_path_native = normalize_to_native(&db_path_unix);
    
    // 読み取り専用の独自の接続を取得（並列アクセス用・メモリ制限付き）
    let conn = state.get_read_only_connection(&db_path_native)?;

    let is_async = matches!(req.query, 
        QueryRequest::GetFilesInModulesAsync { .. } | 
        QueryRequest::SearchFilesInModulesAsync { .. } |
        QueryRequest::GetClassesInModulesAsync { .. }
    );

    tokio::task::spawn_blocking(move || {
        match req.query {
            QueryRequest::GetAssetUsages { asset_path } => {
                {
                    let active_scans = state.active_asset_scans.lock().unwrap();
                    if active_scans.contains(&root_key) {
                        return Ok(json!({ "status": "scanning", "references": [], "derived": [] }));
                    }
                }

                let graphs = state.asset_graphs.lock().unwrap();
                if let Some(graph) = graphs.get(&root_key) {
                    let mut result_refs: HashSet<String> = HashSet::new();
                    let mut result_derived: HashSet<String> = HashSet::new();

                    let class_name = if asset_path.starts_with("/Script/") {
                        asset_path.rfind('.').map(|idx| &asset_path[idx+1..]).unwrap_or(&asset_path)
                    } else {
                        &asset_path
                    };

                    let mut try_names = vec![class_name.to_lowercase()];
                    
                    let prefixes = ['a', 'u', 'f', 'e', 't', 's'];
                    if class_name.len() > 2 {
                        let first = class_name.chars().next().unwrap().to_ascii_lowercase();
                        if prefixes.contains(&first) && class_name.chars().nth(1).unwrap().is_uppercase() {
                            try_names.push(class_name[1..].to_lowercase());
                        }
                    }
                    
                    for name in &try_names {
                        let dot_name = format!(".{}", name);
                        for (k, v) in &graph.references {
                            if k.ends_with(&dot_name) || **k == **name {
                                for x in v { result_refs.insert(x.to_string()); }
                            }
                        }
                        for (k, v) in &graph.derived {
                            if k.ends_with(&dot_name) || **k == **name {
                                for x in v { result_derived.insert(x.to_string()); }
                            }
                        }
                        // Function Call Matching
                        for (k, v) in &graph.functions {
                            if k.ends_with(&dot_name) || **k == **name || k.contains(&format!(":{}", name)) {
                                for x in v { result_refs.insert(x.to_string()); }
                            }
                        }
                    }
                    
                    return Ok(json!({
                        "references": result_refs.into_iter().collect::<Vec<String>>(),
                        "derived": result_derived.into_iter().collect::<Vec<String>>(),
                        "status": "ready"
                    }));
                }
                Ok(json!({ "status": "scanning", "references": [], "derived": [] }))
            }
            QueryRequest::GetAssetDependencies { asset_path } => {
                if asset_path.starts_with("/Script/") { return Ok(json!({ "dependencies": [], "parent_class": null })); }
                let root_path_native = PathBuf::from(normalize_to_native(&req.project_root));
                let rel_path = asset_path.replacen("/Game/", "Content/", 1);
                
                let walker = ignore::WalkBuilder::new(&root_path_native)
                    .hidden(false)
                    .git_ignore(true)
                    .build();
                
                let target_name_uasset = format!("{}.uasset", rel_path.split('/').last().unwrap_or(""));
                let target_name_umap = format!("{}.umap", rel_path.split('/').last().unwrap_or(""));

                let mut target_file = None;
                for entry in walker.filter_map(|e| e.ok()) {
                    let name = entry.file_name().to_str().unwrap_or("");
                    if name == target_name_uasset || name == target_name_umap {
                        let p = entry.path().to_string_lossy().replace('\\', "/");
                        if p.contains(&rel_path) {
                            target_file = Some(entry.path().to_path_buf());
                            break;
                        }
                    }
                }
                
                if let Some(file) = target_file {
                    let mut parser = crate::uasset::UAssetParser::new();
                    if let Ok(_) = parser.parse(&file) {
                        let mut deps = parser.imports;
                        let parent = parser.parent_class;
                        deps.sort();
                        deps.dedup();
                        return Ok(json!({
                            "dependencies": deps,
                            "parent_class": parent
                        }));
                    }
                }
                Ok(json!({ "dependencies": [], "parent_class": null }))
            }
            QueryRequest::FindDerivedClasses { base_class } => {
                {
                    let active_scans = state.active_asset_scans.lock().unwrap();
                    if active_scans.contains(&root_key) {
                        return Ok(json!([{ "name": "Scanning...", "path": "", "symbol_type": "scanning" }]));
                    }
                }

                let mut results = crate::query::process_query(&conn, QueryRequest::FindDerivedClasses { base_class: base_class.clone() })?.as_array().cloned().unwrap_or_default();
                let graphs = state.asset_graphs.lock().unwrap();
                if let Some(graph) = graphs.get(&root_key) {
                    let mut try_names = vec![base_class.to_lowercase()];
                    let prefixes = ['a', 'u', 'f', 'e', 't', 's'];
                    if base_class.len() > 2 {
                        let first = base_class.chars().next().unwrap().to_ascii_lowercase();
                        if prefixes.contains(&first) && base_class.chars().nth(1).unwrap().is_uppercase() {
                            try_names.push(base_class[1..].to_lowercase());
                        }
                    }

                    for name in &try_names {
                        let dot_name = format!(".{}", name);
                        for (k, v) in &graph.derived {
                            if k.ends_with(&dot_name) || **k == **name {
                                for asset in v {
                                    // Case-insensitive duplicate check
                                    let exists = results.iter().any(|r| {
                                        r["path"].as_str().map(|p| p.to_lowercase()) == Some(asset.to_lowercase())
                                    });
                                    if !exists {
                                        results.push(json!({ 
                                            "name": asset.split('/').last().unwrap_or(asset).replace(".uasset", ""), 
                                            "path": asset.to_string(), 
                                            "symbol_type": "uasset" 
                                        }));
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(json!(results))
            }
            QueryRequest::GetAssets => {
                let graphs = state.asset_graphs.lock().unwrap();
                if let Some(graph) = graphs.get(&root_key) {
                    let mut all_assets: HashSet<String> = HashSet::new();
                    for assets in graph.references.values() {
                        for a in assets { all_assets.insert(a.to_string()); }
                    }
                    for assets in graph.derived.values() {
                        for a in assets { all_assets.insert(a.to_string()); }
                    }
                    let mut result: Vec<String> = all_assets.into_iter().collect();
                    result.sort();
                    return Ok(json!(result));
                }
                Ok(json!([]))
            }
            QueryRequest::GetConfigData { engine_root } => {
                let data = crate::query::config::get_config_data_with_cache(
                    &state,
                    &req.project_root,
                    engine_root.as_deref()
                )?;
                Ok(json!(data))
            }
            QueryRequest::GetCompletions { content, line, character, file_path } => {
                let cache = state.get_completion_cache(&root_key);
                crate::completion::process_completion(&conn, &content, line, character, file_path, Some(cache))
            }
            _ => {
                if is_async {
                    let tx_clone = tx.clone();
                    crate::query::process_query_streaming(&conn, req.query, move |items| {
                        let notification = (2, "query/partial", json!({ "msgid": msgid, "items": items }));
                        if let Ok(vec) = rmp_serde::to_vec(&notification) {
                            let mut out = Vec::with_capacity(vec.len() + 4);
                            out.extend_from_slice(&(vec.len() as u32).to_be_bytes());
                            out.extend_from_slice(&vec);
                            let _ = tx_clone.blocking_send(out);
                        }
                        Ok(())
                    })
                } else {
                    crate::query::process_query(&conn, req.query)
                }
            }
        }
    }).await?
}

pub async fn handle_scan(state: &AppState, params: &Value) -> anyhow::Result<Value> {
    let req: ScanRequest = convert_params(params)?;
    let db_path = req.files.get(0).and_then(|f| f.db_path.clone()).ok_or_else(|| anyhow::anyhow!("No DB path"))?;
    let db_path_native = normalize_to_native(&db_path);
    let conn_arc = state.get_connection(&db_path_native)?;
    tokio::task::spawn_blocking(move || {
        let language = tree_sitter_unreal_cpp::LANGUAGE.into();
        let query = tree_sitter::Query::new(&language, scanner::QUERY_STR).unwrap();
        let results: Vec<crate::types::ParseResult> = req.files.into_iter().filter_map(|input| scanner::process_file(&input, &language, &query).ok()).collect();
        let mut conn = conn_arc.lock().unwrap();
        db::save_to_db(&mut conn, &results, Arc::new(crate::types::StdoutReporter))?;
        Ok(serde_json::json!(results.len()))
    }).await?
}

pub async fn get_status(state: &AppState) -> anyhow::Result<Value> {
    let projects = state.projects.lock().unwrap();
    let project_list: Vec<Value> = projects.keys().map(|p| serde_json::json!(p)).collect();
    let clients = state.active_clients.lock().unwrap();
    let client_list: Vec<Value> = clients.iter().map(|&pid| serde_json::json!(pid)).collect();
    Ok(serde_json::json!({ "status": "running", "active_projects": project_list, "active_clients": client_list }))
}

pub async fn list_projects(state: &AppState) -> anyhow::Result<Value> {
    let projects = state.projects.lock().unwrap();
    let list: Vec<Value> = projects.iter().map(|(root, ctx)| {
        serde_json::json!({ "root": root, "db_path": ctx.db_path, "vcs_hash": ctx.vcs_hash })
    }).collect();
    Ok(json!(list))
}
