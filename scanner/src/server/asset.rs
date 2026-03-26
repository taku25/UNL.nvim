use std::sync::{Arc};
use std::path::{Path, PathBuf};
use tracing::{info, warn, debug};
use crate::server::state::{AppState, AssetGraph};
use crate::uasset::UAssetParser;
use crate::server::utils::{normalize_path_key};

pub async fn handle_asset_scan(state: Arc<AppState>, project_root: String) {
    let root_key = normalize_path_key(&project_root);
    
    // ScanGuard: 終了時にフラグを確実に下ろす
    struct ScanGuard { state: Arc<AppState>, root: String }
    impl Drop for ScanGuard {
        fn drop(&mut self) {
            let mut active = self.state.active_asset_scans.lock().unwrap();
            active.remove(&self.root);
            info!("Asset scan flag cleared for: {}", self.root);
        }
    }
    let _guard = ScanGuard { state: state.clone(), root: root_key.clone() };

    info!("Starting targeted asset scan: {:?}", project_root);
    
    let root_path_buf = PathBuf::from(&project_root);
    let result = tokio::task::spawn_blocking(move || {
        let mut graph = AssetGraph::default();
        let mut count = 0;
        let mut error_count = 0;

        let mut content_dirs = Vec::new();
        let walker = ignore::WalkBuilder::new(&root_path_buf)
            .hidden(false).git_ignore(false).follow_links(true).max_depth(Some(4))
            .filter_entry(|entry| {
                let name = entry.file_name().to_str().unwrap_or("");
                !matches!(name, "Intermediate" | "Binaries" | "Build" | "Saved" | ".git" | ".vs")
            }).build();

        for entry in walker.filter_map(|e| e.ok()) {
            if entry.file_name() == "Content" && entry.file_type().map_or(false, |t| t.is_dir()) {
                content_dirs.push(entry.path().to_path_buf());
            }
        }

        for content_dir in content_dirs {
            let walker = ignore::WalkBuilder::new(&content_dir)
                .hidden(false).git_ignore(false).follow_links(true).threads(1).build();

            for entry in walker.filter_map(|e| e.ok()) {
                let path = entry.path();
                let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
                if ext == "uasset" || ext == "umap" {
                    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    let is_essential = ext == "umap" || 
                        filename.starts_with("BP_") || filename.starts_with("ABP_") || 
                        filename.starts_with("WBP_") || filename.starts_with("AM_") || 
                        filename.starts_with("DA_") || filename.starts_with("DT_");

                    if is_essential {
                        match parse_asset_file(path) {
                            Ok((parent, imports, functions)) => {
                                count += 1;
                                let asset_path = to_asset_path(path);
                                add_to_graph(&mut graph, asset_path, parent, imports, functions);
                            }
                            Err(e) => {
                                error_count += 1;
                                warn!("Failed to parse asset: {:?}. Error: {}", path, e);
                            }
                        }
                        if count > 0 && count % 1000 == 0 { debug!("Still scanning assets... {} found.", count); }
                    }
                }
            }
        }
        (graph, count, error_count)
    }).await;

    if let Ok((graph, count, error_count)) = result {
        info!("--- Asset Scan Completed: {:?} ({} files, {} errors) ---", project_root, count, error_count);

        {
            let mut all_graphs = state.asset_graphs.lock().unwrap();
            all_graphs.insert(root_key.clone(), graph);
        }
    }
}

pub async fn update_single_asset(state: Arc<AppState>, project_root: &str, file_path: &Path) {
    let root_key = normalize_path_key(project_root);
    let path_clone = file_path.to_path_buf();
    
    let parse_res = tokio::task::spawn_blocking(move || {
        parse_asset_file(&path_clone).map(|(p, i, f)| (to_asset_path(&path_clone), p, i, f))
    }).await;

    if let Ok(Ok((asset_path, parent, imports, functions))) = parse_res {
        let mut graphs = state.asset_graphs.lock().unwrap();
        if let Some(graph) = graphs.get_mut(&root_key) {
            add_to_graph(graph, asset_path, parent, imports, functions);
            info!("Incremental asset update: {}", file_path.display());
        }
    }
}

fn parse_asset_file(path: &Path) -> anyhow::Result<(Option<String>, Vec<String>, Vec<String>)> {
    let parse_res = std::panic::catch_unwind(move || {
        let mut parser = UAssetParser::new();
        parser.parse(path).map(|_| (parser.parent_class, parser.imports, parser.functions))
    });
    match parse_res {
        Ok(res) => res,
        Err(_) => Err(anyhow::anyhow!("Panic during parse")),
    }
}

fn add_to_graph(graph: &mut AssetGraph, asset_path: String, parent: Option<String>, imports: Vec<String>, functions: Vec<String>) {
    let asset_path_arc: Arc<str> = asset_path.to_lowercase().into();
    if let Some(p) = parent {
        graph.derived.entry(p.to_lowercase().into()).or_default().insert(asset_path_arc.clone());
    }
    for import in imports {
        graph.references.entry(import.to_lowercase().into()).or_default().insert(asset_path_arc.clone());
    }
    for func in functions {
        graph.functions.entry(func.to_lowercase().into()).or_default().insert(asset_path_arc.clone());
    }
}

pub fn to_asset_path(path: &Path) -> String {
    let path_str = path.to_string_lossy().replace('\\', "/");
    if let Some(content_idx) = path_str.find("/Content/") {
        let mut asset_path = "/Game/".to_string();
        let sub_path = &path_str[content_idx + 9..];
        if let Some(dot_idx) = sub_path.rfind('.') { asset_path.push_str(&sub_path[..dot_idx]); } 
        else { asset_path.push_str(sub_path); }
        return asset_path;
    }
    path_str
}
