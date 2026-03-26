use std::fs;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use rayon::prelude::*;
use rusqlite::{params, Connection, OptionalExtension};
use ignore::WalkBuilder;
use regex::Regex;
use tree_sitter::Query;
use crate::types::{RefreshRequest, ModuleDef, ComponentDef, ProgressReporter, InputFile, ParseResult};
use crate::{scanner, db};

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
    
    let normalize_to_native = |s: &str| {
        if cfg!(target_os = "windows") { s.replace('/', "\\") } else { s.replace('\\', "/") }
    };

    let project_root = PathBuf::from(normalize_to_native(&req.project_root));
    let engine_root = req.engine_root.as_ref().map(|r| PathBuf::from(normalize_to_native(r)));
    let db_path_native = normalize_to_native(db_path_str);

    tracing::info!("Starting refresh. Project: {:?}, DB: {}", project_root, db_path_native);

    // UEバージョンの取得
    let ue_version = engine_root.as_ref().and_then(|r| get_ue_version(r));
    if let Some(ref v) = ue_version {
        tracing::info!("Detected Unreal Engine Version: {}.{}.{} ({})", v.MajorVersion, v.MinorVersion, v.PatchVersion, v.BranchName);
    }

    if !project_root.exists() { return Err(anyhow::anyhow!("Project root does not exist: {:?}", project_root)); }

    reporter.report("discovery", 0, 100, &format!("Scanning: {:?}", project_root));

    let project_name = get_name_from_root(&project_root);
    let engine_name = engine_root.as_ref().map(|r| get_name_from_root(r));

    let mut component_defs = Vec::new();
    let mut module_build_files = Vec::new();

    let uproject_path = fs::read_dir(&project_root)?
        .filter_map(|e| e.ok())
        .find(|e| e.path().extension().map_or(false, |ext| ext == "uproject"))
        .map(|e| e.path());

    component_defs.push(ComponentDef {
        name: project_name.clone(), display_name: project_root.file_name().unwrap().to_string_lossy().to_string(),
        comp_type: "Game".to_string(), root_path: project_root.clone(), uproject_path: uproject_path.clone(),
        uplugin_path: None, owner_name: project_name.clone(),
    });

    if let Some(ref eroot) = engine_root {
        component_defs.push(ComponentDef {
            name: engine_name.as_ref().unwrap().clone(), display_name: "Engine".to_string(),
            comp_type: "Engine".to_string(), root_path: eroot.clone(), uproject_path: None, uplugin_path: None,
            owner_name: engine_name.as_ref().unwrap().clone(),
        });
    }

    let mut search_roots = vec![project_root.clone()];
    let scope = req.scope.as_deref().unwrap_or("Full");
    if (scope == "Full" || scope == "Engine") && engine_root.is_some() {
        search_roots.push(engine_root.as_ref().unwrap().clone());
    }

    let excludes: HashSet<String> = req.config.excludes_directory.iter().map(|s| s.to_lowercase()).collect();
    let include_exts: HashSet<String> = req.config.include_extensions.iter().map(|e| e.to_lowercase()).collect();
    let mut files_scanned = 0;
    let mut all_discovered_files = Vec::new();

    for s_root in &search_roots {
        let is_engine = engine_root.as_ref().map_or(false, |er| s_root.starts_with(er));
        let root_owner = if is_engine { engine_name.as_ref().unwrap().clone() } else { project_name.clone() };

        let walker = WalkBuilder::new(s_root).hidden(false).git_ignore(false).filter_entry({
            let excludes = excludes.clone();
            move |entry| { if let Some(name) = entry.file_name().to_str() { if excludes.contains(&name.to_lowercase()) { return false; } } true }
        }).build();

        for entry in walker.filter_map(|e| e.ok()) {
            files_scanned += 1;
            if files_scanned % 1000 == 0 { reporter.report("discovery", 10, 100, &format!("Discovery: {} files seen...", files_scanned)); }
            let path = entry.path();
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            if ext == "uplugin" {
                let plugin_root = path.parent().unwrap();
                let owner = if !is_engine && plugin_root.starts_with(&project_root) { project_name.clone() } else { engine_name.as_ref().cloned().unwrap_or_else(|| "Engine".to_string()) };
                component_defs.push(ComponentDef { name: get_name_from_root(plugin_root), display_name: path.file_stem().unwrap().to_string_lossy().to_string(), comp_type: "Plugin".to_string(), root_path: plugin_root.to_path_buf(), uproject_path: None, uplugin_path: Some(path.to_path_buf()), owner_name: owner });
            } else if path.file_name().map_or(false, |n| n.to_string_lossy().to_lowercase().ends_with(".build.cs")) {
                module_build_files.push((path.to_path_buf(), root_owner.to_string()));
            }
            if entry.file_type().map_or(false, |t| t.is_file()) && include_exts.contains(&ext.to_lowercase()) {
                all_discovered_files.push((normalize_path(path), ext.to_lowercase()));
            }
        }
    }

    let mut unique_components = Vec::new();
    let mut seen_names = HashSet::new();
    for comp in component_defs { if seen_names.insert(comp.name.clone()) { unique_components.push(comp); } }
    let component_defs = unique_components;

    reporter.report("discovery", 40, 100, &format!("Processing {} modules...", module_build_files.len()));
    let mut module_defs = Vec::new();
    let mut seen_module_paths = HashSet::new();
    
    if let Some(ref eroot) = engine_root {
        let e_name = engine_name.as_ref().unwrap();
        module_defs.push(ModuleDef { name: "_EngineConfig".to_string(), path: eroot.join("Engine/Config"), root: eroot.join("Engine/Config"), public_deps: vec![], private_deps: vec![], mod_type: "Config".to_string(), owner_name: e_name.clone(), component_name: Some(e_name.clone()) });
        module_defs.push(ModuleDef { name: "_EngineShaders".to_string(), path: eroot.join("Engine/Shaders"), root: eroot.join("Engine/Shaders"), public_deps: vec![], private_deps: vec![], mod_type: "Shader".to_string(), owner_name: e_name.clone(), component_name: Some(e_name.clone()) });
    }
    module_defs.push(ModuleDef { name: "_GameConfig".to_string(), path: project_root.join("Config"), root: project_root.join("Config"), public_deps: vec![], private_deps: vec![], mod_type: "Config".to_string(), owner_name: project_name.clone(), component_name: Some(project_name.clone()) });

    let mut sorted_components = component_defs.clone();
    sorted_components.sort_by(|a, b| b.root_path.as_os_str().len().cmp(&a.root_path.as_os_str().len()));

    for (path, owner) in module_build_files {
        let root = path.parent().unwrap().to_path_buf();
        let normalized_root = normalize_path(&root);
        if !seen_module_paths.insert(normalized_root) { continue; }
        let name = path.file_name().unwrap().to_string_lossy().split('.').next().unwrap().to_string();
        let (public_deps, private_deps) = parse_build_cs(&path);
        let component_name = sorted_components.iter().find(|c| root.starts_with(&c.root_path)).map(|c| c.name.clone());
        module_defs.push(ModuleDef { name, path, root, public_deps, private_deps, mod_type: "Runtime".to_string(), owner_name: owner, component_name });
    }

    reporter.report("discovery", 60, 100, &format!("Resolving dependencies for {} modules...", module_defs.len()));
    let name_to_def: HashMap<String, &ModuleDef> = module_defs.iter().map(|d| (d.name.clone(), d)).collect();
    
    // メモ化用のキャッシュ: モジュール名 -> そのモジュールが依存する全モジュールセット
    let mut memo: HashMap<String, HashSet<String>> = HashMap::new();
    let mut resolved_modules = Vec::new();
    let total_mods = module_defs.len();

    // 依存関係を再帰的に取得するヘルパー関数
    fn resolve_deep(
        name: &str, 
        name_to_def: &HashMap<String, &ModuleDef>, 
        memo: &mut HashMap<String, HashSet<String>>,
        stack: &mut Vec<String> // 循環参照検知用
    ) -> HashSet<String> {
        if let Some(cached) = memo.get(name) { return cached.clone(); }
        if stack.contains(&name.to_string()) { return HashSet::new(); } // 循環参照
        
        stack.push(name.to_string());
        let mut deps = HashSet::new();
        if let Some(def) = name_to_def.get(name) {
            let mut immediate = Vec::new();
            immediate.extend(def.public_deps.clone());
            immediate.extend(def.private_deps.clone());

            for dep in immediate {
                deps.insert(dep.clone());
                let deep = resolve_deep(&dep, name_to_def, memo, stack);
                for d in deep { deps.insert(d); }
            }
        }
        stack.pop();
        
        memo.insert(name.to_string(), deps.clone());
        deps
    }

    for (i, def) in module_defs.iter().enumerate() {
        let mut stack = Vec::new();
        let deep_deps = resolve_deep(&def.name, &name_to_def, &mut memo, &mut stack);
        
        if (i + 1) % 10 == 0 || i + 1 == total_mods {
            reporter.report("discovery", 60, 100, &format!("Resolving: {}/{} ({})", i + 1, total_mods, def.name));
        }
        resolved_modules.push((def, deep_deps));
    }

    reporter.report("db_sync", 0, 100, "Updating database structure...");
    let db_path = Path::new(&db_path_native);
    db::ensure_correct_version(&db_path_native)?; // ★ 追加: 実行前にバージョンを強制
    let mut conn = Connection::open(db_path)?;
    conn.busy_timeout(std::time::Duration::from_millis(10000))?;
    db::init_db(&conn)?;

    // ★ 追加: UEバージョンの保存
    if let Some(v) = ue_version {
        let _ = conn.execute("INSERT OR REPLACE INTO project_meta (key, value) VALUES ('ue_version_major', ?)", [v.MajorVersion.to_string()]);
        let _ = conn.execute("INSERT OR REPLACE INTO project_meta (key, value) VALUES ('ue_version_minor', ?)", [v.MinorVersion.to_string()]);
        let _ = conn.execute("INSERT OR REPLACE INTO project_meta (key, value) VALUES ('ue_version_patch', ?)", [v.PatchVersion.to_string()]);
        let _ = conn.execute("INSERT OR REPLACE INTO project_meta (key, value) VALUES ('ue_version_branch', ?)", [v.BranchName]);
    }

    let mut string_cache: HashMap<String, i64> = HashMap::new();
    let get_id = |tx: &rusqlite::Transaction, cache: &mut HashMap<String, i64>, text: &str| -> rusqlite::Result<i64> {
        let t = text.trim();
        if let Some(&id) = cache.get(t) { return Ok(id); }
        let id: i64 = match tx.query_row("SELECT id FROM strings WHERE text = ?", [t], |row| row.get(0)) {
            Ok(id) => id,
            Err(_) => { tx.execute("INSERT INTO strings (text) VALUES (?)", [t])?; tx.last_insert_rowid() }
        };
        cache.insert(t.to_string(), id);
        Ok(id)
    };

    let mut existing_mtimes = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT s.text, f.mtime FROM files f JOIN strings s ON f.path_id = s.id")?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
        for r in rows { if let Ok((p, m)) = r { existing_mtimes.insert(p, m); } }
    }

    conn.execute("PRAGMA foreign_keys = OFF", [])?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    for table in ["components", "modules"] { let _ = tx.execute(&format!("DELETE FROM {}", table), []); }
    tx.commit()?;
    conn.execute("PRAGMA foreign_keys = ON", [])?;

    let mut mod_id_map = HashMap::new();
    let tx = conn.transaction()?;
    for comp in &component_defs {
        tx.execute("INSERT OR REPLACE INTO components (name, display_name, type, owner_name, root_path, uplugin_path, uproject_path) VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![comp.name, comp.display_name, comp.comp_type, comp.owner_name, normalize_path(&comp.root_path), comp.uplugin_path.as_ref().map(|p| normalize_path(p)), comp.uproject_path.as_ref().map(|p| normalize_path(p))],
        )?;
    }
    for (def, deep_deps) in &resolved_modules {
        let deep_deps_json = serde_json::to_string(&deep_deps.iter().collect::<Vec<_>>()).unwrap();
        let name_id = get_id(&tx, &mut string_cache, &def.name)?;
        let root_id = get_id(&tx, &mut string_cache, &normalize_path(&def.root))?;
        tx.execute("INSERT OR REPLACE INTO modules (name_id, type, scope, root_path_id, build_cs_path, owner_name, component_name, deep_dependencies) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![name_id, def.mod_type, "Individual", root_id, normalize_path(&def.path), def.owner_name, def.component_name, deep_deps_json],
        )?;
        mod_id_map.insert(normalize_path(&def.root), tx.last_insert_rowid());
    }
    let project_root_str = normalize_path(&project_root);
    let global_mod_id = {
        let name_id = get_id(&tx, &mut string_cache, "_Global")?;
        let root_id = get_id(&tx, &mut string_cache, &project_root_str)?;
        tx.execute("INSERT OR REPLACE INTO modules (name_id, type, scope, root_path_id) VALUES (?, ?, ?, ?)", params![name_id, "Global", "Game", root_id])?;
        tx.last_insert_rowid()
    };
    tx.commit()?;

    let mut sorted_roots: Vec<(String, i64)> = mod_id_map.into_iter().collect();
    sorted_roots.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    let mut headers_to_parse = Vec::new();
    let mut other_files = Vec::new();
    let mut current_on_disk = HashSet::new();

    // ファイル更新のトランザクション開始
    let tx = conn.transaction()?;
    {
        for (path_str, ext) in all_discovered_files {
            current_on_disk.insert(path_str.clone());
            let mod_id = sorted_roots.iter().find(|(r, _)| path_str.starts_with(r)).map(|(_, id)| *id).unwrap_or(global_mod_id);
            let mtime = fs::metadata(&path_str).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0) as i64;
            
            if let Some(&old_mtime) = existing_mtimes.get(&path_str) {
                if old_mtime == mtime {
                    // mtimeが変わっていない場合でも、module_idだけ更新（高速）
                    let path_id = match string_cache.get(&path_str) {
                        Some(&id) => id,
                        None => {
                            let id: i64 = tx.query_row("SELECT id FROM strings WHERE text = ?", [&path_str], |r| r.get(0)).optional()?.unwrap_or_else(|| 0);
                            id
                        }
                    };
                    if path_id > 0 {
                        tx.execute("UPDATE files SET module_id = ? WHERE path_id = ? AND module_id != ?", params![mod_id, path_id, mod_id])?;
                    }
                    continue;
                }
            }

            if ext == "h" || ext == "hpp" {
                headers_to_parse.push(InputFile { path: path_str, mtime: mtime as u64, old_hash: None, module_id: Some(mod_id), db_path: None });
            } else {
                other_files.push((path_str, mtime, mod_id, ext));
            }
        }
    }
    tx.commit()?; // ここで一括コミット

    // 削除されたファイルの処理
    let tx = conn.transaction()?;
    {
        for (path, _) in &existing_mtimes {
            if !current_on_disk.contains(path) {
                let path_id = tx.query_row("SELECT id FROM strings WHERE text = ?", [path], |r| r.get(0)).optional()?.unwrap_or(0);
                if path_id > 0 {
                    tx.execute("DELETE FROM files WHERE path_id = ?", params![path_id])?;
                }
            }
        }
    }
    tx.commit()?;

    if !headers_to_parse.is_empty() {
        reporter.report("analysis", 0, headers_to_parse.len(), &format!("Analyzing {} changed headers...", headers_to_parse.len()));
        let language = tree_sitter_unreal_cpp::LANGUAGE.into();
        let query = Arc::new(Query::new(&language, scanner::QUERY_STR).expect("Failed to parse query"));
        let processed_count = Arc::new(AtomicUsize::new(0));
        let total = headers_to_parse.len();
        let results: Vec<ParseResult> = headers_to_parse.into_par_iter().map(|input| {
            let res = scanner::process_file(&input, &language, &query).unwrap_or_else(|_| ParseResult { path: input.path, status: "error".to_string(), mtime: input.mtime, data: None, module_id: input.module_id });
            let current = processed_count.fetch_add(1, Ordering::Relaxed) + 1;
            if current % 20 == 0 || current == total { reporter.report("analysis", current, total, &format!("Analyzing: {}/{}", current, total)); }
            res
        }).collect();
        db::save_to_db(&mut conn, &results, Arc::clone(&reporter))?;
    }

    if !other_files.is_empty() {
        let tx = conn.transaction()?;
        {
            for (path, mtime, mod_id, ext) in other_files {
                let filename = Path::new(&path).file_name().and_then(|s| s.to_str()).unwrap_or("unknown");
                let path_id = get_id(&tx, &mut string_cache, &path)?;
                let filename_id = get_id(&tx, &mut string_cache, filename)?;
                // INSERT OR REPLACE files
                tx.execute("INSERT OR REPLACE INTO files (path_id, filename_id, extension, mtime, module_id, is_header) VALUES (?, ?, ?, ?, ?, 0)", params![path_id, filename_id, ext, mtime as i64, mod_id])?;
            }
        }
        tx.commit()?;
    }

    reporter.report("complete", 100, 100, "Refresh complete.");
    Ok(())
}

fn normalize_path(path: &Path) -> String { path.to_string_lossy().replace(char::from(92), "/") }
fn get_name_from_root(path: &Path) -> String { path.file_name().and_then(|s| s.to_str()).unwrap_or("Unknown").to_string() }
fn parse_build_cs(path: &Path) -> (Vec<String>, Vec<String>) {
    let content = fs::read_to_string(path).unwrap_or_default();
    let mut public_deps = Vec::new();
    let mut private_deps = Vec::new();
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