use std::fs;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use rayon::prelude::*;
use rusqlite::{params, Connection};
use ignore::WalkBuilder;
use regex::Regex;
use tree_sitter::Query;
use crate::types::{RefreshRequest, ModuleDef, ComponentDef, UProjectPluginJson, report_progress, InputFile, ParseResult};
use crate::scanner;
use crate::db;

pub fn run_refresh(req: RefreshRequest) -> anyhow::Result<()> {
    let project_root = Path::new(&req.project_root);
    let engine_root = req.engine_root.as_ref().map(|r| Path::new(r));

    let project_name = get_name_from_root(project_root);
    let engine_name = engine_root.map(|r| get_name_from_root(r));

    // 1. Discover Components (Game, Engine, Plugins)
    report_progress("discovery", 0, 100, "Scanning components...");
    let mut component_defs = Vec::new();

    // Game Component
    let uproject_path = fs::read_dir(project_root)?
        .filter_map(|e| e.ok())
        .find(|e| e.path().extension().map_or(false, |ext| ext == "uproject"))
        .map(|e| e.path());

    component_defs.push(ComponentDef {
        name: project_name.clone(),
        display_name: project_root.file_name().unwrap().to_string_lossy().to_string(),
        comp_type: "Game".to_string(),
        root_path: project_root.to_path_buf(),
        uproject_path: uproject_path.clone(),
        uplugin_path: None,
        owner_name: project_name.clone(),
    });

    // Engine Component
    if let Some(eroot) = engine_root {
        component_defs.push(ComponentDef {
            name: engine_name.as_ref().unwrap().clone(),
            display_name: "Engine".to_string(),
            comp_type: "Engine".to_string(),
            root_path: eroot.to_path_buf(),
            uproject_path: None,
            uplugin_path: None,
            owner_name: engine_name.as_ref().unwrap().clone(),
        });
    }

    // Discover Plugins
    let mut search_roots = vec![project_root.to_path_buf()];
    
    // Determine scope
    let scope = req.scope.as_deref().unwrap_or("Full");
    if scope == "Full" || scope == "Engine" {
        if let Some(r) = engine_root { search_roots.push(r.to_path_buf()); }
    }
    if scope == "Engine" {
        search_roots = vec![];
        if let Some(r) = engine_root { search_roots.push(r.to_path_buf()); }
    }

    // Prepare exclude filter
    let excludes: HashSet<String> = req.config.excludes_directory.iter().map(|s| s.to_lowercase()).collect();
    let filter = move |entry: &ignore::DirEntry| -> bool {
        if let Some(name) = entry.file_name().to_str() {
            if excludes.contains(&name.to_lowercase()) {
                return false;
            }
        }
        true
    };

    for s_root in &search_roots {
        let walker = WalkBuilder::new(s_root)
            .filter_entry(filter.clone())
            .build();
        for entry in walker.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "uplugin") {
                let plugin_root = path.parent().unwrap();
                let owner = if plugin_root.starts_with(project_root) { project_name.clone() } else { engine_name.as_ref().unwrap().clone() };
                component_defs.push(ComponentDef {
                    name: get_name_from_root(plugin_root),
                    display_name: path.file_stem().unwrap().to_string_lossy().to_string(),
                    comp_type: "Plugin".to_string(),
                    root_path: plugin_root.to_path_buf(),
                    uproject_path: None,
                    uplugin_path: Some(path.to_path_buf()),
                    owner_name: owner,
                });
            }
        }
    }

    // Sort components by root path length (longest first) for better mapping
    let mut sorted_components = component_defs.clone();
    sorted_components.sort_by(|a, b| b.root_path.as_os_str().len().cmp(&a.root_path.as_os_str().len()));

    // 2. Discover Modules and Pseudo-modules
    report_progress("discovery", 50, 100, "Scanning modules...");
    let mut module_defs = Vec::new();

    // Add Pseudo-modules (Config, Shaders, etc.)
    if let Some(eroot) = engine_root {
        module_defs.push(ModuleDef {
            name: "_EngineConfig".to_string(),
            path: eroot.join("Engine/Config"),
            root: eroot.join("Engine/Config"),
            public_deps: vec![], private_deps: vec![],
            mod_type: "Config".to_string(),
            owner_name: engine_name.as_ref().unwrap().clone(),
            component_name: Some(engine_name.as_ref().unwrap().clone()),
        });
        module_defs.push(ModuleDef {
            name: "_EngineShaders".to_string(),
            path: eroot.join("Engine/Shaders"),
            root: eroot.join("Engine/Shaders"),
            public_deps: vec![], private_deps: vec![],
            mod_type: "Shader".to_string(),
            owner_name: engine_name.as_ref().unwrap().clone(),
            component_name: Some(engine_name.as_ref().unwrap().clone()),
        });
    }
    module_defs.push(ModuleDef {
        name: "_GameConfig".to_string(),
        path: project_root.join("Config"),
        root: project_root.join("Config"),
        public_deps: vec![], private_deps: vec![],
        mod_type: "Config".to_string(),
        owner_name: project_name.clone(),
        component_name: Some(project_name.clone()),
    });
    if project_root.join("Shaders").exists() {
        module_defs.push(ModuleDef {
            name: "_GameShaders".to_string(),
            path: project_root.join("Shaders"),
            root: project_root.join("Shaders"),
            public_deps: vec![], private_deps: vec![],
            mod_type: "Shader".to_string(),
            owner_name: project_name.clone(),
            component_name: Some(project_name.clone()),
        });
    }
    
    let mut type_map = get_module_type_map(project_root);
    if let Some(eroot) = engine_root {
        let engine_types = get_module_type_map(eroot);
        for (k, v) in engine_types { type_map.entry(k).or_insert(v); }
    }

    for s_root in &search_roots {
        let walker = WalkBuilder::new(s_root)
            .filter_entry(filter.clone())
            .build();
        for entry in walker.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.file_name().map_or(false, |n| n.to_string_lossy().to_lowercase().ends_with(".build.cs")) {
                let name = path.file_name().unwrap().to_string_lossy().split('.').next().unwrap().to_string();
                let (pub_deps, priv_deps) = parse_build_cs(path);
                
                let mod_type = type_map.get(&name).cloned().unwrap_or_else(|| "Runtime".to_string());
                let owner = if path.starts_with(project_root) { "Game" } else { "Engine" };

                let root = path.parent().unwrap();
                let component_name = sorted_components.iter()
                    .find(|c| root.starts_with(&c.root_path))
                    .map(|c| c.name.clone());

                module_defs.push(ModuleDef {
                    name,
                    path: path.to_path_buf(),
                    root: root.to_path_buf(),
                    public_deps: pub_deps,
                    private_deps: priv_deps,
                    mod_type,
                    owner_name: owner.to_string(),
                    component_name,
                });
            }
        }
    }
    report_progress("discovery", 100, 100, &format!("Found {} modules.", module_defs.len()));

    // 3. Resolve Dependencies
    let total_mods = module_defs.len();
    report_progress("discovery", 100, 100, &format!("Resolving dependencies: 0/{}", total_mods));
    
    let name_to_def: HashMap<String, &ModuleDef> = module_defs.iter().map(|d| (d.name.clone(), d)).collect();
    let mut resolved_modules = Vec::new();

    for (i, def) in module_defs.iter().enumerate() {
        if total_mods > 10 && i % 10 == 0 {
            report_progress("discovery", 100, 100, &format!("Resolving dependencies: {}/{}", i, total_mods));
        }
        let mut deep_deps = HashSet::new();
        let mut queue = vec![def.name.clone()];
        let mut visited = HashSet::new();

        while let Some(current) = queue.pop() {
            if !visited.insert(current.clone()) { continue; }
            if let Some(d) = name_to_def.get(&current) {
                for dep in &d.public_deps { deep_deps.insert(dep.clone()); queue.push(dep.clone()); }
                for dep in &d.private_deps { deep_deps.insert(dep.clone()); queue.push(dep.clone()); }
            }
        }
        deep_deps.remove(&def.name);
        resolved_modules.push((def, deep_deps));
    }
    report_progress("discovery", 100, 100, "Dependencies resolved.");

    // 4. Sync to DB
    report_progress("db_sync", 0, 100, "Opening database for sync...");
    let mut conn = Connection::open(&req.db_path)?;
    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
    let _ = conn.pragma_update(None, "journal_mode", "WAL");
    
    // Ensure tables exist
    db::init_db(&conn)?;

    report_progress("db_sync", 10, 100, "Starting transaction...");
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

    // 外部キー制約を一時的に無効化して高速化 & ハング回避
    tx.execute("PRAGMA foreign_keys = OFF", [])?;

    tx.execute("DELETE FROM enum_values", [])?;
    tx.execute("DELETE FROM inheritance", [])?;
    tx.execute("DELETE FROM members", [])?;
    tx.execute("DELETE FROM classes", [])?;
    tx.execute("DELETE FROM files", [])?;
    tx.execute("DELETE FROM modules", [])?;
    tx.execute("DELETE FROM components", [])?;

    tx.execute("PRAGMA foreign_keys = ON", [])?;

    // Save Components
    report_progress("db_sync", 30, 100, "Saving components...");
    for comp in &component_defs {
        tx.execute(
            "INSERT INTO components (name, display_name, type, owner_name, root_path, uplugin_path, uproject_path) VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                comp.name,
                comp.display_name,
                comp.comp_type,
                comp.owner_name,
                normalize_path(&comp.root_path),
                comp.uplugin_path.as_ref().map(|p| normalize_path(p)),
                comp.uproject_path.as_ref().map(|p| normalize_path(p)),
            ],
        )?;
    }

    let mut mod_id_map = HashMap::new();
    let total_mods = resolved_modules.len();
    report_progress("db_sync", 40, 100, &format!("Saving modules: 0/{}", total_mods));

    for (i, (def, deep_deps)) in resolved_modules.iter().enumerate() {
        if i % 10 == 0 {
            report_progress("db_sync", 40 + (i * 50 / total_mods), 100, &format!("Saving modules: {}/{}", i, total_mods));
        }
        let deep_deps_json = serde_json::to_string(&deep_deps.iter().collect::<Vec<_>>()).unwrap();
        let root_str = normalize_path(&def.root);
        tx.execute(
            "INSERT INTO modules (name, type, scope, root_path, build_cs_path, owner_name, component_name, deep_dependencies) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![def.name, def.mod_type, "Individual", root_str, normalize_path(&def.path), def.owner_name, def.component_name, deep_deps_json],
        )?;
        let id = tx.last_insert_rowid();
        mod_id_map.insert(root_str, id);
    }
    
    let global_mod_id = {
        tx.execute(
            "INSERT OR IGNORE INTO modules (name, type, scope, root_path) VALUES (?, ?, ?, ?)",
            params!["_Global", "Global", "Game", normalize_path(project_root)],
        )?;
        tx.last_insert_rowid()
    };

    report_progress("db_sync", 95, 100, "Committing changes...");
    tx.commit()?;
    report_progress("db_sync", 100, 100, "Module structure saved.");

    // 5. Scan All Files and Prepare Analysis
    report_progress("file_scan", 0, 100, "Scanning Files...");
    let include_exts: HashSet<String> = req.config.include_extensions.iter().map(|e| e.to_lowercase()).collect();
    let mut sorted_roots: Vec<String> = mod_id_map.keys().cloned().collect();
    sorted_roots.sort_by(|a, b| b.len().cmp(&a.len()));

    let mut headers_to_parse = Vec::new();
    let mut other_files = Vec::new();

    for s_root in &search_roots {
        let walker = WalkBuilder::new(s_root)
            .filter_entry(filter.clone())
            .build();
        
        let mut file_count = 0;
        for entry in walker.filter_map(|e| e.ok()) {
            file_count += 1;
            if file_count % 5000 == 0 {
                report_progress("file_scan", file_count, file_count, &format!("Scanning Files: Found {}", file_count));
            }

            if entry.file_type().map_or(false, |t| t.is_file()) {
                let path = entry.path();
                let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                
                if include_exts.contains(&ext) {
                    let path_str = normalize_path(path);
                    let mut mod_id = global_mod_id;
                    for root in &sorted_roots {
                        if path_str.starts_with(root) {
                            mod_id = *mod_id_map.get(root).unwrap();
                            break;
                        }
                    }

                    let mtime = fs::metadata(path).and_then(|m| m.modified()).ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs()).unwrap_or(0);

                    if ext == "h" || ext == "hpp" {
                        headers_to_parse.push(InputFile {
                            path: path_str,
                            mtime,
                            old_hash: None, // TODO: Pull from DB for incremental
                            module_id: Some(mod_id),
                            db_path: None,
                        });
                    } else {
                        other_files.push((path_str, mtime, mod_id, ext));
                    }
                }
            }
        }
    }

    // 6. Parallel Analysis
    let total_headers = headers_to_parse.len();
    report_progress("analysis", 0, total_headers, &format!("Analyzing headers (0/{})", total_headers));
    
    let language = tree_sitter_unreal_cpp::LANGUAGE.into();
    let query = Arc::new(Query::new(&language, scanner::QUERY_STR).expect("Failed to parse query"));
    let processed_count = Arc::new(AtomicUsize::new(0));

    let results: Vec<ParseResult> = headers_to_parse.into_par_iter().map(|input| {
        let res = scanner::process_file(&input, &language, &query).unwrap_or_else(|_| ParseResult {
            path: input.path,
            status: "error".to_string(),
            mtime: input.mtime,
            data: None,
            module_id: input.module_id,
        });
        
        let current = processed_count.fetch_add(1, Ordering::Relaxed) + 1;
        if current % 100 == 0 || current == total_headers {
            report_progress("analysis", current, total_headers, &format!("Analyzing headers ({}/{})", current, total_headers));
        }
        res
    }).collect();

    // 7. Final DB Sync (Files, Classes, Members)
    report_progress("db_sync", 0, 100, "Finalizing database...");
    db::save_to_db(&req.db_path, &results)?;

    // Sync non-header files too
    let mut conn = Connection::open(&req.db_path)?;
    let tx = conn.transaction()?;
    for (path, mtime, mod_id, ext) in other_files {
        let filename = Path::new(&path).file_name().and_then(|s| s.to_str()).unwrap_or("unknown");
        let _ = tx.execute("DELETE FROM files WHERE path = ?", params![path]);
        let _ = tx.execute(
            "INSERT INTO files (path, filename, extension, mtime, module_id, is_header) VALUES (?, ?, ?, ?, ?, 0)",
            params![path, filename, ext, mtime as i64, mod_id],
        );
    }
    tx.commit()?;

    report_progress("complete", 100, 100, "Refresh complete.");
    Ok(())
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace(char::from(92), "/")
}

fn get_name_from_root(path: &Path) -> String {
    normalize_path(path).replace("/", "_").replace(":", "_")
}

fn parse_build_cs(path: &Path) -> (Vec<String>, Vec<String>) {
    let content = fs::read_to_string(path).unwrap_or_default();
    let mut public_deps = Vec::new();
    let mut private_deps = Vec::new();

    // Use [ ] instead of \s, [.] instead of \. to avoid backslashes
    let re_add_range = Regex::new(r"(Public|Private)DependencyModuleNames[.]AddRange[ \t]*[(][ \t]*new[ \t]+string[ \t]*\[\][ \t]*[\{](.*?)[\}][ \t]*[)]").unwrap();
    let re_add = Regex::new("(Public|Private)DependencyModuleNames[.]Add[ \t]*[(][ \t]*\"(.*?)\" [ \t]*[)]").unwrap();
    let re_quoted = Regex::new("\"(.*?)\" ").unwrap();

    for cap in re_add_range.captures_iter(&content) {
        let list_type = &cap[1];
        let names_blob = &cap[2];
        for name_cap in re_quoted.captures_iter(names_blob) {
            let name = name_cap[1].to_string();
            if list_type == "Public" { public_deps.push(name); } else { private_deps.push(name); }
        }
    }

    for cap in re_add.captures_iter(&content) {
        let list_type = &cap[1];
        let name = cap[2].to_string();
        if list_type == "Public" { public_deps.push(name); } else { private_deps.push(name); }
    }

    (public_deps, private_deps)
}

fn get_module_type_map(root: &Path) -> HashMap<String, String> {
    let mut type_map = HashMap::new();
    let walker = WalkBuilder::new(root)
        .max_depth(Some(4))
        .build();

    for entry in walker.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "uproject" || ext == "uplugin") {
            if let Ok(content) = fs::read_to_string(path) {
                if let Ok(json) = serde_json::from_str::<UProjectPluginJson>(&content) {
                    if let Some(modules) = json.modules {
                        for m in modules {
                            type_map.insert(m.name, m.mod_type);
                        }
                    }
                }
            }
        }
    }
    type_map
}