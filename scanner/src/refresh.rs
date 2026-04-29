use std::fs;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use rayon::prelude::*;
use rusqlite::{params, Connection};
use ignore::{WalkBuilder, WalkState};
use regex::Regex;
use tree_sitter::Query;
use crate::types::{RefreshRequest, ModuleDef, ComponentDef, ProgressReporter, PhaseInfo, InputFile, ParseResult};
use crate::{scanner, db, vcs};
use crate::db::path::get_or_create_directory;
use crate::vcs::ChangedFiles;

#[derive(serde::Deserialize, Debug)]
#[allow(non_snake_case)]
struct UeBuildVersion {
    MajorVersion: i32,
    MinorVersion: i32,
    PatchVersion: i32,
    BranchName: String,
}

fn get_ue_version(engine_root: &Path) -> Option<UeBuildVersion> {
    let version_path = engine_root.join("Engine/Build/Build.version");
    if let Ok(content) = fs::read_to_string(version_path) {
        return serde_json::from_str(&content).ok();
    }
    None
}

pub fn run_refresh(req: RefreshRequest, reporter: Arc<dyn ProgressReporter>) -> anyhow::Result<()> {
    let db_path_str = req.db_path.as_ref().ok_or_else(|| anyhow::anyhow!("DB path required for refresh"))?;
    let normalize_to_native = |s: &str| { if cfg!(target_os = "windows") { s.replace('/', "\\") } else { s.replace('\\', "/") } };
    let project_root = PathBuf::from(normalize_to_native(&req.project_root));
    let engine_root = req.engine_root.as_ref().map(|r| PathBuf::from(normalize_to_native(r)));
    let db_path_native = normalize_to_native(db_path_str);

    let ue_version = engine_root.as_ref().and_then(|r| get_ue_version(r));
    if !project_root.exists() { return Err(anyhow::anyhow!("Project root does not exist: {:?}", project_root)); }

    // === VCS Integration: determine whether engine scan can be skipped ===
    // Read stored revisions before walk (DB may not exist yet on first run).
    let stored_engine_rev: Option<String> = Connection::open(Path::new(&db_path_native)).ok()
        .and_then(|c| c.query_row("SELECT value FROM project_meta WHERE key = 'vcs_engine_revision'", [], |r| r.get::<_, String>(0)).ok());
    let stored_game_rev: Option<String> = Connection::open(Path::new(&db_path_native)).ok()
        .and_then(|c| c.query_row("SELECT value FROM project_meta WHERE key = 'vcs_game_revision'", [], |r| r.get::<_, String>(0)).ok());

    // Detect VCS providers for game and engine roots.
    let game_vcs = vcs::detect(&project_root);
    let current_game_rev = game_vcs.current_revision(&project_root);
    // Engine may be a git submodule with its own .git ref, so detect independently.
    let current_engine_rev = engine_root.as_ref().and_then(|er| vcs::detect(er).current_revision(er));

    // === Incremental game refresh path ===
    // When the game VCS revision changed but only non-structural files were modified,
    // skip the full walk and only re-parse the files reported by `changed_since`.
    // Structural changes (.build.cs / .uplugin / .uproject) still trigger a full scan
    // because they may add/remove modules or plugins.
    let game_rev_changed = match (&stored_game_rev, &current_game_rev) {
        (Some(stored), Some(current)) => stored != current,
        _ => false,
    };
    if game_rev_changed {
        if let Some(stored_rev) = &stored_game_rev {
            if let Some(changed) = game_vcs.changed_since(&project_root, stored_rev) {
                let is_structural = changed.modified.iter().chain(changed.deleted.iter()).any(|p| {
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
                    name.ends_with(".build.cs") || name.ends_with(".uplugin") || name.ends_with(".uproject")
                });
                // Also guard: if DB version doesn't match, a full rescan is required.
                let db_version_ok = db_version_matches(&db_path_native);
                if !is_structural && db_version_ok {
                    tracing::info!(
                        "Incremental game refresh: {} modified, {} deleted file(s). Skipping full walk.",
                        changed.modified.len(), changed.deleted.len()
                    );
                    return run_incremental_game_refresh(
                        &req, reporter, &project_root, changed, &db_path_native, current_game_rev
                    );
                }
            }
        }
    }

    // Skip engine walk when its VCS revision is identical to the stored one.
    let engine_rev_same = match (&stored_engine_rev, &current_engine_rev) {
        (Some(stored), Some(current)) => stored == current,
        _ => false,
    };

    // Send the phase plan first so the Lua client can build its progress UI
    // without any hardcoded weights on its side.
    reporter.report_plan(&[
        PhaseInfo { name: "discovery".into(),  label: "Discovery".into(),  weight: if engine_rev_same { 0.02 } else { 0.05 } },
        PhaseInfo { name: "db_sync".into(),    label: "DB Sync".into(),    weight: 0.15 },
        PhaseInfo { name: "analysis".into(),   label: "Analysis".into(),   weight: 0.65 },
        PhaseInfo { name: "finalizing".into(), label: "Finalizing".into(), weight: 0.15 },
    ]);

    reporter.report("discovery", 0, 100, &format!("Scanning: {:?}", project_root));
    if engine_rev_same {
        reporter.report("discovery", 0, 100, &format!("Engine revision unchanged ({}), skipping engine scan.", current_engine_rev.as_deref().unwrap_or("?")));
    }

    let project_name = get_name_from_root(&project_root);
    let engine_name = engine_root.as_ref().map(|r| get_name_from_root(r));
    let mut component_defs = Vec::new();

    let uproject_path = fs::read_dir(&project_root)?.filter_map(|e| e.ok()).find(|e| e.path().extension().is_some_and(|ext| ext == "uproject")).map(|e| e.path());
    component_defs.push(ComponentDef { name: project_name.clone(), display_name: project_root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| project_name.clone()), comp_type: "Game".to_string(), root_path: project_root.clone(), uproject_path: uproject_path.clone(), uplugin_path: None, owner_name: project_name.clone() });

    if let Some(ref eroot) = engine_root {
        // Only register engine component when we are actually scanning it.
        // When engine_rev_same, the component entry from the previous scan is
        // preserved in the DB (we skip the unconditional DELETE below).
        if !engine_rev_same {
            component_defs.push(ComponentDef { name: engine_name.as_ref().unwrap().clone(), display_name: "Engine".to_string(), comp_type: "Engine".to_string(), root_path: eroot.clone(), uproject_path: None, uplugin_path: None, owner_name: engine_name.as_ref().unwrap().clone() });
        }
    }

    let mut search_roots = vec![project_root.clone()];
    // Only walk the engine when its revision changed (or VCS is unavailable).
    // When skipped, engine files and modules are preserved from the previous DB state.
    if !engine_rev_same && (req.scope.as_deref().unwrap_or("Full") == "Full" || req.scope.as_deref().unwrap_or("Full") == "Engine") {
        if let Some(ref root) = engine_root {
            search_roots.push(root.clone());
        }
    }
    // Normalised walked-root strings used later to scope the cleanup pass.
    let walked_root_strs: Vec<String> = search_roots.iter().map(|r| normalize_path(r)).collect();

    let excludes: HashSet<String> = req.config.excludes_directory.iter().map(|s| s.to_lowercase()).collect();
    let include_exts: HashSet<String> = req.config.include_extensions.iter().map(|e| e.to_lowercase()).collect();

    // ① Parallel walk: collect discovered files, plugins, and build.cs files concurrently
    let all_discovered_files: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let module_build_files: Arc<Mutex<Vec<(PathBuf, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let plugin_components: Arc<Mutex<Vec<ComponentDef>>> = Arc::new(Mutex::new(Vec::new()));
    let files_scanned = Arc::new(AtomicUsize::new(0));

    let excludes_a = Arc::new(excludes);
    let include_exts_a = Arc::new(include_exts);
    let engine_root_a: Arc<Option<PathBuf>> = Arc::new(engine_root.clone());
    let engine_name_a: Arc<Option<String>> = Arc::new(engine_name.clone());
    let project_root_a = Arc::new(project_root.clone());
    let project_name_a = Arc::new(project_name.clone());

    let mut builder = WalkBuilder::new(&search_roots[0]);
    for root in search_roots.iter().skip(1) { builder.add(root); }
    builder.hidden(false).git_ignore(false);

    {
        let excludes = Arc::clone(&excludes_a);
        builder.filter_entry(move |e| {
            if let Some(name) = e.file_name().to_str() {
                if excludes.contains(&name.to_lowercase()) { return false; }
            }
            true
        });
    }

    builder.build_parallel().run(|| {
        let adf       = Arc::clone(&all_discovered_files);
        let mbf       = Arc::clone(&module_build_files);
        let pc        = Arc::clone(&plugin_components);
        let counter   = Arc::clone(&files_scanned);
        let exts      = Arc::clone(&include_exts_a);
        let er        = Arc::clone(&engine_root_a);
        let en        = Arc::clone(&engine_name_a);
        let pr        = Arc::clone(&project_root_a);
        let pn        = Arc::clone(&project_name_a);
        let reporter  = Arc::clone(&reporter);

        Box::new(move |result| {
            let entry = match result { Ok(e) => e, Err(_) => return WalkState::Continue };
            let count = counter.fetch_add(1, Ordering::Relaxed) + 1;
            if count % 5000 == 0 {
                reporter.report("discovery", 10, 100, &format!("Discovery: {} files seen...", count));
            }
            let path = entry.path();
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
            let root_owner = if er.as_ref().as_ref().is_some_and(|er| path.starts_with(er)) {
                en.as_ref().as_ref().cloned().unwrap_or_else(|| "Engine".to_string())
            } else {
                pn.as_ref().clone()
            };

            if ext == "uplugin" {
                if let Some(plugin_root) = path.parent() {
                    let owner = if plugin_root.starts_with(pr.as_ref()) { pn.as_ref().clone() }
                                else { en.as_ref().as_ref().cloned().unwrap_or_else(|| "Engine".to_string()) };
                    let comp = ComponentDef {
                        name: get_name_from_root(plugin_root),
                        display_name: path.file_stem().unwrap_or_default().to_string_lossy().to_string(),
                        comp_type: "Plugin".to_string(),
                        root_path: plugin_root.to_path_buf(),
                        uproject_path: None,
                        uplugin_path: Some(path.to_path_buf()),
                        owner_name: owner,
                    };
                    pc.lock().push(comp);
                }
            } else if path.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().ends_with(".build.cs")) {
                mbf.lock().push((path.to_path_buf(), root_owner));
            }

            if entry.file_type().is_some_and(|t| t.is_file()) && exts.contains(&ext) {
                adf.lock().push((normalize_path(path), ext));
            }
            WalkState::Continue
        })
    });

    // Merge parallel results
    let all_discovered_files = Arc::try_unwrap(all_discovered_files).unwrap().into_inner();
    let module_build_files   = Arc::try_unwrap(module_build_files).unwrap().into_inner();
    {
        let mut found = plugin_components.lock();
        component_defs.extend(found.drain(..));
    }

    let mut seen_names = HashSet::new();
    let component_defs: Vec<_> = component_defs.into_iter().filter(|c| seen_names.insert(c.name.clone())).collect();

    let mut module_defs = Vec::new();
    if let Some(ref eroot) = engine_root {
        // Only re-add engine static modules when we are scanning the engine.
        // When engine_rev_same, these entries remain in the DB unchanged.
        if !engine_rev_same {
            let e_name = engine_name.as_ref().unwrap();
            module_defs.push(ModuleDef { name: "_EngineConfig".to_string(), path: eroot.join("Engine/Config"), root: eroot.join("Engine/Config"), public_deps: vec![], private_deps: vec![], mod_type: "Config".to_string(), owner_name: e_name.clone(), component_name: Some(e_name.clone()) });
            module_defs.push(ModuleDef { name: "_EngineShaders".to_string(), path: eroot.join("Engine/Shaders"), root: eroot.join("Engine/Shaders"), public_deps: vec![], private_deps: vec![], mod_type: "Shader".to_string(), owner_name: e_name.clone(), component_name: Some(e_name.clone()) });
        }
    }
    module_defs.push(ModuleDef { name: "_GameConfig".to_string(), path: project_root.join("Config"), root: project_root.join("Config"), public_deps: vec![], private_deps: vec![], mod_type: "Config".to_string(), owner_name: project_name.clone(), component_name: Some(project_name.clone()) });

    let mut sorted_components = component_defs.clone();
    sorted_components.sort_by(|a, b| b.root_path.as_os_str().len().cmp(&a.root_path.as_os_str().len()));

    let mut seen_module_paths = HashSet::new();
    for (path, owner) in module_build_files {
        let root = path.parent().unwrap().to_path_buf();
        if !seen_module_paths.insert(normalize_path(&root)) { continue; }
        let (public_deps, private_deps) = parse_build_cs(&path);
        let component_name = sorted_components.iter().find(|c| root.starts_with(&c.root_path)).map(|c| c.name.clone());
        module_defs.push(ModuleDef { name: path.file_name().unwrap().to_string_lossy().split('.').next().unwrap().to_string(), path, root, public_deps, private_deps, mod_type: "Runtime".to_string(), owner_name: owner, component_name });
    }

    let name_to_def: HashMap<String, &ModuleDef> = module_defs.iter().map(|d| (d.name.clone(), d)).collect();
    let mut memo: HashMap<String, HashSet<String>> = HashMap::new();
    let mut resolved_modules = Vec::new();
    for (i, def) in module_defs.iter().enumerate() {
        let mut stack = Vec::new();
        let deep_deps = resolve_deep(&def.name, &name_to_def, &mut memo, &mut stack);
        if (i + 1) % 50 == 0 || i + 1 == module_defs.len() { reporter.report("discovery", 60, 100, &format!("Resolving: {}/{} ({})", i + 1, module_defs.len(), def.name)); }
        resolved_modules.push((def, deep_deps));
    }

    reporter.report("db_sync", 0, 100, "Updating database structure...");
    db::ensure_correct_version(&db_path_native)?; 
    let mut conn = Connection::open(Path::new(&db_path_native))?;
    conn.busy_timeout(std::time::Duration::from_millis(10000))?;
    db::init_db(&conn)?;
    // FK ON にすることで孤立レコードが残らないようにする。
    // modules 削除前に files.module_id を NULL 化しているので FK 違反は起きない。
    conn.execute("PRAGMA foreign_keys = ON", [])?;

    if let Some(v) = ue_version {
        for (k, val) in [("major", v.MajorVersion.to_string()), ("minor", v.MinorVersion.to_string()), ("patch", v.PatchVersion.to_string()), ("branch", v.BranchName)] {
            let _ = conn.execute("INSERT OR REPLACE INTO project_meta (key, value) VALUES (?, ?)", [format!("ue_version_{}", k), val]);
        }
    }

    let mut string_cache = HashMap::new();
    let mut dir_cache = HashMap::new();

    // ② Load all directory entries once, then reconstruct paths in Rust — avoids recursive PATH_CTE.
    // dir_map is kept alive so we can also reconstruct engine module root paths below.
    let mut dir_map: HashMap<i64, (Option<i64>, String)> = HashMap::new();
    let mut existing_mtimes = HashMap::new();
    {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap_or(0);
        if count > 0 {
            {
                let mut stmt = conn.prepare(
                    "SELECT d.id, d.parent_id, s.text FROM directories d JOIN strings s ON d.name_id = s.id"
                )?;
                let rows = stmt.query_map([], |row| Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, String>(2)?,
                )))?;
                for r in rows.flatten() { dir_map.insert(r.0, (r.1, r.2)); }
            }
            let mut stmt = conn.prepare(
                "SELECT f.directory_id, s.text, f.mtime FROM files f JOIN strings s ON f.filename_id = s.id"
            )?;
            let rows = stmt.query_map([], |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            )))?;
            for r in rows.flatten() {
                let (dir_id, filename, mtime) = r;
                let full = reconstruct_path(&dir_map, dir_id, &filename);
                existing_mtimes.insert(full, mtime);
            }
        }
    }

    // When engine scan is skipped, load the existing engine module IDs from the
    // DB so that game files can still be assigned to the correct module.
    let engine_mod_ids: HashMap<String, i64> = if engine_rev_same && engine_name.is_some() {
        let mut stmt = conn.prepare(
            "SELECT m.id, m.root_directory_id FROM modules m WHERE m.owner_name = ?"
        )?;
        let rows = stmt.query_map(params![engine_name.as_ref().unwrap()], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut map = HashMap::new();
        for r in rows.flatten() {
            let (mod_id, root_dir_id) = r;
            let path = reconstruct_dir_path(&dir_map, root_dir_id);
            map.insert(path, mod_id);
        }
        map
    } else {
        HashMap::new()
    };

    let tx = conn.transaction()?;
    // When the engine revision is unchanged, preserve engine components/modules
    // in the DB — they are expensive to rebuild and nothing has changed.
    // FK ON 状態での module_id チェックを回避するため、削除前に files.module_id を NULL にしておく。
    match (engine_rev_same, engine_name.as_ref()) {
        (true, Some(en)) => {
            tx.execute("UPDATE files SET module_id = NULL WHERE module_id IN (SELECT id FROM modules WHERE owner_name != ?)", params![en])?;
            tx.execute("DELETE FROM components WHERE owner_name != ?", params![en])?;
            tx.execute("DELETE FROM modules WHERE owner_name != ?", params![en])?;
        }
        _ => {
            tx.execute("UPDATE files SET module_id = NULL", [])?;
            tx.execute("DELETE FROM components", [])?;
            tx.execute("DELETE FROM modules", [])?;
        }
    }
    
    let mut mod_id_map = HashMap::new();
    for comp in &component_defs {
        tx.execute("INSERT INTO components (name, display_name, type, owner_name, root_path, uplugin_path, uproject_path) VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![comp.name, comp.display_name, comp.comp_type, comp.owner_name, normalize_path(&comp.root_path), comp.uplugin_path.as_ref().map(|p| normalize_path(p)), comp.uproject_path.as_ref().map(|p| normalize_path(p))],
        )?;
    }
    for (def, deep_deps) in &resolved_modules {
        let name_id = db::get_or_create_string(&tx, &mut string_cache, &def.name)?;
        let root_dir_id = get_or_create_directory(&tx, &mut string_cache, &mut dir_cache, &def.root)?;
        tx.execute("INSERT INTO modules (name_id, type, scope, root_directory_id, build_cs_path, owner_name, component_name, deep_dependencies) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![name_id, def.mod_type, "Individual", root_dir_id, normalize_path(&def.path), def.owner_name, def.component_name, serde_json::to_string(&deep_deps.iter().collect::<Vec<_>>()).unwrap()],
        )?;
        mod_id_map.insert(normalize_path(&def.root), tx.last_insert_rowid());
    }
    let global_mod_id = {
        let name_id = db::get_or_create_string(&tx, &mut string_cache, "_Global")?;
        let root_dir_id = get_or_create_directory(&tx, &mut string_cache, &mut dir_cache, &project_root)?;
        tx.execute("INSERT INTO modules (name_id, type, scope, root_directory_id) VALUES (?, ?, ?, ?)", params![name_id, "Global", "Game", root_dir_id])?;
        tx.last_insert_rowid()
    };
    tx.commit()?;

    // Merge preserved engine module IDs so that file→module assignment is correct.
    mod_id_map.extend(engine_mod_ids);

    let mut sorted_roots: Vec<_> = mod_id_map.into_iter().collect();
    sorted_roots.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    let mut files_to_parse = Vec::new();
    let mut other_files = Vec::new();
    let mut current_on_disk = HashSet::new();

    for (path_str, ext) in all_discovered_files {
        current_on_disk.insert(path_str.clone());
        let mod_id = sorted_roots.iter().find(|(r, _)| path_str.starts_with(r)).map(|(_, id)| *id).unwrap_or(global_mod_id);
        let mtime = fs::metadata(&path_str).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0) as i64;
        
        let mut needs_parse = true;
        if let Some(&old_mtime) = existing_mtimes.get(&path_str) {
            if old_mtime == mtime {
                needs_parse = false;
            }
        }

        if needs_parse && ["h", "hpp", "cpp", "cc", "c", "inl"].contains(&ext.as_str()) {
            files_to_parse.push(InputFile { path: path_str, mtime: mtime as u64, old_hash: None, module_id: Some(mod_id), db_path: None });
        } else {
            other_files.push((path_str, mtime, mod_id, ext));
        }
    }

    let tx = conn.transaction()?;
    for path in existing_mtimes.keys() {
        // Only clean up files that belong to a root we actually walked.
        // Engine files are intentionally preserved when engine scan is skipped.
        let in_walked_root = walked_root_strs.iter().any(|r| path.starts_with(r.as_str()));
        if in_walked_root && !current_on_disk.contains(path) {
            let p = Path::new(path);
            let dir_id = get_or_create_directory(&tx, &mut string_cache, &mut dir_cache, p.parent().unwrap_or(Path::new("")))?;
            let fn_id = db::get_or_create_string(&tx, &mut string_cache, p.file_name().unwrap().to_str().unwrap())?;
            tx.execute("DELETE FROM files WHERE directory_id = ? AND filename_id = ?", params![dir_id, fn_id])?;
        }
    }
    tx.commit()?;

    if !files_to_parse.is_empty() {
        reporter.report("analysis", 0, files_to_parse.len(), &format!("Analyzing {} files...", files_to_parse.len()));
        let language = tree_sitter_unreal_cpp::LANGUAGE.into();
        let query = Arc::new(Query::new(&language, scanner::QUERY_STR).expect("Failed to parse query"));
        let include_query = Arc::new(Query::new(&language, scanner::INCLUDE_QUERY_STR).expect("Failed to parse include query"));
        let processed_count = Arc::new(AtomicUsize::new(0));
        let total = files_to_parse.len();
        let results: Vec<ParseResult> = files_to_parse.into_par_iter().map(|input| {
            let res = scanner::process_file(&input, &language, &query, &include_query).unwrap_or_else(|_| ParseResult { path: input.path, status: "error".to_string(), mtime: input.mtime, data: None, module_id: input.module_id });
            let current = processed_count.fetch_add(1, Ordering::Relaxed) + 1;
            if current % 50 == 0 || current == total { reporter.report("analysis", current, total, &format!("Analyzing: {}/{}", current, total)); }
            res
        }).collect();
        db::save_to_db(&mut conn, &results, Arc::clone(&reporter))?;
    }

    if !other_files.is_empty() {
        let tx = conn.transaction()?;
        for (path, mtime, mod_id, ext) in other_files {
            let p = Path::new(&path);
            let dir_id = get_or_create_directory(&tx, &mut string_cache, &mut dir_cache, p.parent().unwrap_or(Path::new("")))?;
            let fn_id = db::get_or_create_string(&tx, &mut string_cache, p.file_name().unwrap().to_str().unwrap())?;
            // INSERT OR REPLACE で module_id だけ更新されるようにする
            tx.execute("INSERT OR REPLACE INTO files (directory_id, filename_id, extension, mtime, module_id, is_header) VALUES (?, ?, ?, ?, ?, ?)", 
                params![dir_id, fn_id, ext, { mtime }, mod_id, if ext == "h" || ext == "hpp" { 1 } else { 0 }])?;
        }
        tx.commit()?;
    }

    reporter.report("complete", 100, 100, "Refresh complete.");

    // Persist VCS revisions so the next refresh can detect unchanged roots.
    if let Some(ref rev) = current_game_rev {
        let _ = conn.execute("INSERT OR REPLACE INTO project_meta (key, value) VALUES (?, ?)", params!["vcs_game_revision", rev]);
    }
    if let Some(ref rev) = current_engine_rev {
        let _ = conn.execute("INSERT OR REPLACE INTO project_meta (key, value) VALUES (?, ?)", params!["vcs_engine_revision", rev]);
    }

    Ok(())
}

fn normalize_path(path: &Path) -> String { path.to_string_lossy().replace(char::from(92), "/") }
fn get_name_from_root(path: &Path) -> String { path.file_name().and_then(|s| s.to_str()).unwrap_or("Unknown").to_string() }

/// Reconstruct a slash-separated full path from the in-memory directory map.
fn reconstruct_path(dir_map: &HashMap<i64, (Option<i64>, String)>, mut dir_id: i64, filename: &str) -> String {
    let mut segments: Vec<String> = vec![filename.to_string()];
    loop {
        match dir_map.get(&dir_id) {
            Some((Some(parent_id), name)) => { segments.push(name.clone()); dir_id = *parent_id; }
            Some((None, name))            => { segments.push(name.clone()); break; }
            None                          => break,
        }
    }
    segments.reverse();
    segments.join("/")
}

/// Reconstruct a slash-separated directory path (no trailing filename) from the dir_map.
fn reconstruct_dir_path(dir_map: &HashMap<i64, (Option<i64>, String)>, mut dir_id: i64) -> String {
    let mut segments: Vec<String> = vec![];
    loop {
        match dir_map.get(&dir_id) {
            Some((Some(parent_id), name)) => { segments.push(name.clone()); dir_id = *parent_id; }
            Some((None, name))            => { segments.push(name.clone()); break; }
            None                          => break,
        }
    }
    segments.reverse();
    segments.join("/")
}
fn resolve_deep(name: &str, name_to_def: &HashMap<String, &ModuleDef>, memo: &mut HashMap<String, HashSet<String>>, stack: &mut Vec<String>) -> HashSet<String> {
    if let Some(cached) = memo.get(name) { return cached.clone(); }
    if stack.contains(&name.to_string()) { return HashSet::new(); }
    stack.push(name.to_string());
    let mut deps = HashSet::new();
    if let Some(def) = name_to_def.get(name) {
        for dep in def.public_deps.iter().chain(def.private_deps.iter()) {
            deps.insert(dep.clone());
            for d in resolve_deep(dep, name_to_def, memo, stack) { deps.insert(d); }
        }
    }
    stack.pop();
    memo.insert(name.to_string(), deps.clone());
    deps
}
fn parse_build_cs(path: &Path) -> (Vec<String>, Vec<String>) {
    let content = fs::read_to_string(path).unwrap_or_default();
    let mut public_deps = Vec::new(); let mut private_deps = Vec::new();
    let re_add_range = Regex::new(r"(Public|Private)DependencyModuleNames[.]AddRange[ \t]*[(][ \t]*new[ \t]+string[ \t]*[]][ \t]*[{](.*?)[}][ \t]*[)]").unwrap();
    let re_add = Regex::new("(Public|Private)DependencyModuleNames[.]Add[ \t]*[(][ \t]*\"(.*?)\"[ \t]*[)]").unwrap();
    let re_quoted = Regex::new("\"(.*?)\"").unwrap();
    for cap in re_add_range.captures_iter(&content) {
        let list_type = &cap[1];
        for name_cap in re_quoted.captures_iter(&cap[2]) { if list_type == "Public" { public_deps.push(name_cap[1].to_string()); } else { private_deps.push(name_cap[1].to_string()); } }
    }
    for cap in re_add.captures_iter(&content) { if &cap[1] == "Public" { public_deps.push(cap[2].to_string()); } else { private_deps.push(cap[2].to_string()); } }
    (public_deps, private_deps)
}

/// Check if the on-disk DB matches the current DB_VERSION without side effects.
fn db_version_matches(db_path: &str) -> bool {
    Connection::open(Path::new(db_path)).ok()
        .and_then(|c| c.query_row(
            "SELECT value FROM project_meta WHERE key = 'db_version'",
            [], |r| r.get::<_, String>(0),
        ).ok())
        .and_then(|v| v.parse::<i32>().ok())
        .map_or(false, |v| v == db::DB_VERSION)
}

/// Incremental game refresh: re-parse only files reported by VCS `changed_since`.
/// Called when the game revision changed but no structural files (.build.cs / .uplugin /
/// .uproject) were affected, so modules and components are unchanged.
fn run_incremental_game_refresh(
    req: &RefreshRequest,
    reporter: Arc<dyn ProgressReporter>,
    _project_root: &Path,
    changed: ChangedFiles,
    db_path_native: &str,
    current_game_rev: Option<String>,
) -> anyhow::Result<()> {
    reporter.report_plan(&[
        PhaseInfo { name: "analysis".into(), label: "Analysis".into(), weight: 1.0 },
    ]);
    reporter.report("analysis", 0, 100, &format!(
        "Incremental: {} modified, {} deleted file(s).",
        changed.modified.len(), changed.deleted.len()
    ));

    let mut conn = Connection::open(Path::new(db_path_native))?;
    conn.busy_timeout(std::time::Duration::from_millis(10000))?;
    // FK ON にすることで DELETE FROM files が子テーブル (file_includes/classes/members) をカスケード削除する。
    // save_to_db が内部で FK OFF にして一括 INSERT し、終了後に再び FK ON に戻す。
    conn.execute("PRAGMA foreign_keys = ON", [])?;

    // Rebuild the in-memory directory map so we can look up or create path IDs.
    let mut dir_map: HashMap<i64, (Option<i64>, String)> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT d.id, d.parent_id, s.text FROM directories d JOIN strings s ON d.name_id = s.id"
        )?;
        let rows = stmt.query_map([], |row| Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<i64>>(1)?,
            row.get::<_, String>(2)?,
        )))?;
        for r in rows.flatten() { dir_map.insert(r.0, (r.1, r.2)); }
    }

    // Load module-root → module_id mapping (longest-prefix match used below).
    let mut mod_id_map: Vec<(String, i64)> = {
        let mut stmt = conn.prepare(
            "SELECT m.id, m.root_directory_id FROM modules m"
        )?;
        let rows = stmt.query_map([], |row| Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
        )))?;
        rows.flatten().map(|(mod_id, root_dir_id)| {
            let path = reconstruct_dir_path(&dir_map, root_dir_id);
            (path, mod_id)
        }).collect()
    };
    mod_id_map.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    let global_mod_id: i64 = conn.query_row(
        "SELECT m.id FROM modules m JOIN strings s ON m.name_id = s.id WHERE s.text = '_Global'",
        [], |r| r.get(0),
    ).unwrap_or(0);

    let mut string_cache: HashMap<String, i64> = HashMap::new();
    let mut dir_cache: HashMap<(Option<i64>, i64), i64> = HashMap::new();

    // Delete removed files from the DB.
    {
        let tx = conn.transaction()?;
        for path in &changed.deleted {
            let path_unix = normalize_path(path);
            let p = Path::new(&path_unix);
            if let (Some(parent), Some(filename)) = (p.parent(), p.file_name().and_then(|n| n.to_str())) {
                if let (Ok(dir_id), Ok(fn_id)) = (
                    get_or_create_directory(&tx, &mut string_cache, &mut dir_cache, parent),
                    db::get_or_create_string(&tx, &mut string_cache, filename),
                ) {
                    tx.execute(
                        "DELETE FROM files WHERE directory_id = ? AND filename_id = ?",
                        params![dir_id, fn_id],
                    )?;
                }
            }
        }
        tx.commit()?;
    }

    let include_exts: HashSet<String> = req.config.include_extensions.iter()
        .map(|e| e.to_lowercase()).collect();
    let parseable = ["h", "hpp", "cpp", "cc", "c", "inl"];

    let mut files_to_parse: Vec<InputFile> = Vec::new();
    for path in &changed.modified {
        if !path.exists() { continue; }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if !include_exts.contains(&ext) { continue; }
        if !parseable.contains(&ext.as_str()) { continue; }
        let path_unix = normalize_path(path);
        let mtime = std::fs::metadata(path)
            .and_then(|m| m.modified()).ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs()).unwrap_or(0);
        let mod_id = mod_id_map.iter()
            .find(|(r, _)| path_unix.starts_with(r.as_str()))
            .map(|(_, id)| *id)
            .unwrap_or(global_mod_id);
        files_to_parse.push(InputFile {
            path: path_unix,
            mtime,
            old_hash: None,
            module_id: Some(mod_id),
            db_path: None,
        });
    }

    let total = files_to_parse.len();
    if total > 0 {
        reporter.report("analysis", 0, total, &format!("Re-parsing {} changed file(s)...", total));
        let language = tree_sitter_unreal_cpp::LANGUAGE.into();
        let query = Arc::new(Query::new(&language, scanner::QUERY_STR).expect("query"));
        let include_query = Arc::new(Query::new(&language, scanner::INCLUDE_QUERY_STR).expect("include_query"));
        let processed_count = Arc::new(AtomicUsize::new(0));
        let results: Vec<ParseResult> = files_to_parse.into_par_iter().map(|input| {
            let res = scanner::process_file(&input, &language, &query, &include_query)
                .unwrap_or_else(|_| ParseResult {
                    path: input.path.clone(), status: "error".to_string(),
                    mtime: input.mtime, data: None, module_id: input.module_id,
                });
            let current = processed_count.fetch_add(1, Ordering::Relaxed) + 1;
            reporter.report("analysis", current, total, &format!("Re-parsing: {}/{}", current, total));
            res
        }).collect();
        db::save_to_db(&mut conn, &results, Arc::clone(&reporter))?;
    }

    reporter.report("complete", 100, 100,
        &format!("Incremental refresh complete ({} file(s) updated).", total));

    if let Some(ref rev) = current_game_rev {
        conn.execute(
            "INSERT OR REPLACE INTO project_meta (key, value) VALUES (?, ?)",
            params!["vcs_game_revision", rev],
        )?;
    }

    Ok(())
}

