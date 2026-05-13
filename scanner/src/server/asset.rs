use std::sync::Arc;
use std::path::{Path, PathBuf};
use std::collections::HashSet;
use std::time::UNIX_EPOCH;
use tracing::{info, warn};
use crate::server::state::{AppState, AssetGraph};
use crate::server::asset_db::{self, AssetCacheRow};
use crate::uasset::UAssetParser;
use crate::server::utils::normalize_path_key;

pub async fn handle_asset_scan(state: Arc<AppState>, project_root: String) {
    let root_key = normalize_path_key(&project_root);

    // ScanGuard: 終了時にフラグを確実に下ろす
    struct ScanGuard { state: Arc<AppState>, root: String }
    impl Drop for ScanGuard {
        fn drop(&mut self) {
            self.state.active_asset_scans.lock().remove(&self.root);
            info!("Asset scan flag cleared for: {}", self.root);
        }
    }
    let _guard = ScanGuard { state: state.clone(), root: root_key.clone() };

    // Derive the asset cache DB path from the main symbol DB path.
    let asset_db_path_str = {
        let projects = state.projects.lock();
        let Some(ctx) = projects.get(&root_key) else {
            warn!("No project context for {} — skipping asset scan", root_key);
            return;
        };
        // ctx.db_path is unix-normalized; convert to native for the FS.
        let native = crate::server::utils::normalize_to_native(&ctx.db_path);
        asset_db::asset_db_path(&native)
    };

    info!("Starting targeted asset scan: {:?} (cache: {})", project_root, asset_db_path_str);

    let root_path_buf = PathBuf::from(&project_root);
    let result = tokio::task::spawn_blocking(move || {
        // ── Open / create the asset cache DB ────────────────────────────────
        let mut conn = asset_db::open_asset_db(&asset_db_path_str)?;

        // ── Load existing cache: file_path (native) → row ───────────────────
        let mut cached = asset_db::load_all_by_path(&conn)?;
        info!("Asset cache: {} entries loaded", cached.len());

        // ── Walk filesystem ──────────────────────────────────────────────────
        let mut content_dirs: Vec<PathBuf> = Vec::new();
        let walker = ignore::WalkBuilder::new(&root_path_buf)
            .hidden(false).git_ignore(false).follow_links(true).max_depth(Some(4))
            .filter_entry(|e| {
                let name = e.file_name().to_str().unwrap_or("");
                !matches!(name, "Intermediate" | "Binaries" | "Build" | "Saved" | ".git" | ".vs")
            })
            .build();
        for entry in walker.filter_map(|e| e.ok()) {
            if entry.file_name() == "Content" && entry.file_type().is_some_and(|t| t.is_dir()) {
                content_dirs.push(entry.path().to_path_buf());
            }
        }

        let mut seen: HashSet<String> = HashSet::new();
        let mut new_rows: Vec<AssetCacheRow> = Vec::new();
        let mut n_skipped = 0usize;
        let mut n_parsed  = 0usize;
        let mut n_errors  = 0usize;

        for content_dir in content_dirs {
            let walker = ignore::WalkBuilder::new(&content_dir)
                .hidden(false).git_ignore(false).follow_links(true).threads(1).build();

            for entry in walker.filter_map(|e| e.ok()) {
                let path = entry.path();
                let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
                if ext != "uasset" && ext != "umap" { continue; }

                let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let is_essential = ext == "umap"
                    || filename.starts_with("BP_")  || filename.starts_with("ABP_")
                    || filename.starts_with("WBP_") || filename.starts_with("AM_")
                    || filename.starts_with("DA_")  || filename.starts_with("DT_");
                if !is_essential { continue; }

                let fp_native = path.to_string_lossy().to_string();
                seen.insert(fp_native.clone());

                let current_mtime = path.metadata().ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);

                // Cache hit: mtime unchanged → no re-parse needed
                if let Some(row) = cached.get(&fp_native) {
                    if row.mtime == current_mtime && current_mtime > 0 {
                        n_skipped += 1;
                        continue;
                    }
                }

                // Parse
                match parse_asset_file(path) {
                    Ok((parent, imports, functions)) => {
                        n_parsed += 1;
                        new_rows.push(AssetCacheRow {
                            file_path:    fp_native,
                            asset_path:   to_asset_path(path),
                            mtime:        current_mtime,
                            parent_class: parent,
                            imports,
                            functions,
                        });
                    }
                    Err(e) => {
                        n_errors += 1;
                        warn!("Failed to parse asset: {:?}. Error: {}", path, e);
                    }
                }

                if (n_parsed + n_errors) % 1000 == 0 {
                    info!("Asset scan in progress: {} parsed, {} cached…", n_parsed, n_skipped);
                }
            }
        }

        // ── Persist new / changed rows ───────────────────────────────────────
        asset_db::upsert_batch(&mut conn, &new_rows)?;

        // Apply new rows to the cached map so build_graph sees the latest data.
        for row in new_rows {
            cached.insert(row.file_path.clone(), row);
        }

        // ── Remove stale entries (deleted files) ─────────────────────────────
        let stale: Vec<String> = cached.keys()
            .filter(|fp| !seen.contains(*fp))
            .cloned()
            .collect();
        asset_db::delete_batch(&mut conn, &stale)?;
        for fp in &stale { cached.remove(fp); }

        // ── Build in-memory graph from the final merged cache ─────────────────
        let graph = asset_db::build_graph(&cached);

        info!(
            "Asset scan done: {} parsed, {} from cache, {} stale deleted, {} errors",
            n_parsed, n_skipped, stale.len(), n_errors
        );
        Ok::<_, anyhow::Error>((graph, n_parsed, n_skipped))
    }).await;

    match result {
        Ok(Ok((graph, parsed, skipped))) => {
            info!("--- Asset Scan Completed: {:?} ({} parsed, {} from cache) ---",
                  project_root, parsed, skipped);
            state.asset_graphs.lock().insert(root_key, graph);
        }
        Ok(Err(e)) => warn!("Asset scan error for {}: {}", project_root, e),
        Err(e)     => warn!("Asset scan task panic for {}: {}", project_root, e),
    }
}

pub async fn update_single_asset(state: Arc<AppState>, project_root: &str, file_path: &Path) {
    let root_key      = normalize_path_key(project_root);
    let path_clone    = file_path.to_path_buf();

    // Derive asset DB path
    let asset_db_path_str = {
        let projects = state.projects.lock();
        projects.get(&root_key).map(|ctx| {
            let native = crate::server::utils::normalize_to_native(&ctx.db_path);
            asset_db::asset_db_path(&native)
        })
    };

    let parse_res = tokio::task::spawn_blocking(move || {
        let r = parse_asset_file(&path_clone)
            .map(|(p, i, f)| (to_asset_path(&path_clone), p, i, f));

        // Get mtime
        let mtime = path_clone.metadata().ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        r.map(|(ap, parent, imports, functions)| {
            (path_clone, ap, parent, imports, functions, mtime)
        })
    }).await;

    if let Ok(Ok((path_native, asset_path, parent, imports, functions, mtime))) = parse_res {
        let fp_native = path_native.to_string_lossy().to_string();

        // Persist to asset cache DB
        if let Some(ref db_path) = asset_db_path_str {
            if let Ok(conn) = asset_db::open_asset_db(db_path) {
                let row = AssetCacheRow {
                    file_path:    fp_native.clone(),
                    asset_path:   asset_path.clone(),
                    mtime,
                    parent_class: parent.clone(),
                    imports:      imports.clone(),
                    functions:    functions.clone(),
                };
                if let Err(e) = asset_db::upsert_one(&conn, &row) {
                    warn!("Failed to persist asset update for {}: {}", fp_native, e);
                }
            }
        }

        // Update in-memory graph
        let mut graphs = state.asset_graphs.lock();
        if let Some(graph) = graphs.get_mut(&root_key) {
            add_to_graph(graph, asset_path, parent, imports, functions);
            info!("Incremental asset update: {}", fp_native);
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
        Err(_)  => Err(anyhow::anyhow!("Panic during parse")),
    }
}

fn add_to_graph(
    graph: &mut AssetGraph,
    asset_path: String,
    parent:     Option<String>,
    imports:    Vec<String>,
    functions:  Vec<String>,
) {
    let ap: Arc<str> = asset_path.to_lowercase().into();
    if let Some(p) = parent {
        graph.derived.entry(p.to_lowercase().into()).or_default().insert(ap.clone());
    }
    for import in imports {
        graph.references.entry(import.to_lowercase().into()).or_default().insert(ap.clone());
    }
    for func in functions {
        graph.functions.entry(func.to_lowercase().into()).or_default().insert(ap.clone());
    }
}

pub fn to_asset_path(path: &Path) -> String {
    let path_str = path.to_string_lossy().replace('\\', "/");
    if let Some(idx) = path_str.find("/Content/") {
        let mut ap = "/Game/".to_string();
        let sub = &path_str[idx + 9..];
        if let Some(dot) = sub.rfind('.') { ap.push_str(&sub[..dot]); }
        else { ap.push_str(sub); }
        return ap;
    }
    path_str
}
