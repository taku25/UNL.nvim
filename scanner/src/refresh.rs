use std::fs;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use rayon::prelude::*;
use rusqlite::{params, Connection};
use ignore::WalkBuilder;
use regex::Regex;
use tree_sitter::Query;
use crate::types::{RefreshRequest, ModuleDef, ComponentDef, ProgressReporter, InputFile, ParseResult};
use crate::{scanner, db};

pub fn run_refresh(req: RefreshRequest, reporter: Arc<dyn ProgressReporter>) -> anyhow::Result<()> {
    let db_path_str = req.db_path.as_ref().ok_or_else(|| anyhow::anyhow!("DB path required for refresh"))?;
    
    // パスの完全正規化
    let sep = std::path::MAIN_SEPARATOR.to_string();
    let project_root = PathBuf::from(req.project_root.replace('/', &sep));
    let engine_root = req.engine_root.as_ref().map(|r| PathBuf::from(r.replace('/', &sep)));

    if !project_root.exists() {
        return Err(anyhow::anyhow!("Project root does not exist: {:?}", project_root));
    }

    reporter.report("discovery", 0, 100, &format!("Scanning: {:?}", project_root));

    let project_name = get_name_from_root(&project_root);
    let engine_name = engine_root.as_ref().map(|r| get_name_from_root(r));

    // 1. Discover Components and Modules in one pass
    reporter.report("discovery", 5, 100, "Scanning project structure...");
    let mut component_defs = Vec::new();
    let mut module_build_files = Vec::new();

    // Base Components
    let uproject_path = fs::read_dir(&project_root)?
        .filter_map(|e| e.ok())
        .find(|e| e.path().extension().map_or(false, |ext| ext == "uproject"))
        .map(|e| e.path());

    component_defs.push(ComponentDef {
        name: project_name.clone(),
        display_name: project_root.file_name().unwrap().to_string_lossy().to_string(),
        comp_type: "Game".to_string(),
        root_path: project_root.clone(),
        uproject_path: uproject_path.clone(),
        uplugin_path: None,
        owner_name: project_name.clone(),
    });

    if let Some(ref eroot) = engine_root {
        component_defs.push(ComponentDef {
            name: engine_name.as_ref().unwrap().clone(),
            display_name: "Engine".to_string(),
            comp_type: "Engine".to_string(),
            root_path: eroot.clone(),
            uproject_path: None,
            uplugin_path: None,
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

        let walker = WalkBuilder::new(s_root)
            .hidden(false)
            .filter_entry({
                let excludes = excludes.clone();
                move |entry| {
                    if let Some(name) = entry.file_name().to_str() {
                        if excludes.contains(&name.to_lowercase()) { return false; }
                    }
                    true
                }
            })
            .build();

        for entry in walker.filter_map(|e| e.ok()) {
            files_scanned += 1;
            if files_scanned % 500 == 0 {
                reporter.report("discovery", 10, 100, &format!("Discovery: {} files seen...", files_scanned));
            }

            let path = entry.path();
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");

            if ext == "uplugin" {
                let plugin_root = path.parent().unwrap();
                let owner = if !is_engine && plugin_root.starts_with(&project_root) { project_name.clone() } else { engine_name.as_ref().cloned().unwrap_or_else(|| "Engine".to_string()) };
                component_defs.push(ComponentDef {
                    name: get_name_from_root(plugin_root),
                    display_name: path.file_stem().unwrap().to_string_lossy().to_string(),
                    comp_type: "Plugin".to_string(),
                    root_path: plugin_root.to_path_buf(),
                    uproject_path: None,
                    uplugin_path: Some(path.to_path_buf()),
                    owner_name: owner,
                });
            } else if path.file_name().map_or(false, |n| n.to_string_lossy().to_lowercase().ends_with(".build.cs")) {
                module_build_files.push((path.to_path_buf(), root_owner.to_string()));
            }
            
            // Collect files for scanning in the SAME pass
            if entry.file_type().map_or(false, |t| t.is_file()) && include_exts.contains(&ext.to_lowercase()) {
                all_discovered_files.push((normalize_path(path), ext.to_lowercase()));
            }
        }
    }

    // Deduplicate components
    let mut unique_components = Vec::new();
    let mut seen_names = HashSet::new();
    for comp in component_defs {
        if seen_names.insert(comp.name.clone()) {
            unique_components.push(comp);
        }
    }
    let component_defs = unique_components;

    // 2. Process Modules
    reporter.report("discovery", 40, 100, &format!("Processing {} modules...", module_build_files.len()));
    let mut module_defs = Vec::new();
    let mut seen_module_paths = HashSet::new();
    
    // Add Pseudo-modules
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

        module_defs.push(ModuleDef {
            name, path, root, public_deps, private_deps,
            mod_type: "Runtime".to_string(), owner_name: owner, component_name,
        });
    }

    // 3. Resolve Dependencies
    let total_mods = module_defs.len();
    reporter.report("discovery", 60, 100, &format!("Resolving dependencies for {} modules...", total_mods));
    let name_to_def: HashMap<String, &ModuleDef> = module_defs.iter().map(|d| (d.name.clone(), d)).collect();
    let mut resolved_modules = Vec::new();

    for (i, def) in module_defs.iter().enumerate() {
        if i % 50 == 0 { reporter.report("discovery", 60 + (i * 20 / total_mods), 100, &format!("Resolving deps: {}/{}", i, total_mods)); }
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

    // 4. Sync to DB
    reporter.report("db_sync", 0, 100, "Updating database structure...");
    let db_path = Path::new(db_path_str);
    let mut conn = Connection::open(db_path)?;
    conn.busy_timeout(std::time::Duration::from_millis(10000))?;
    db::init_db(&conn)?;

    // Load existing mtimes to skip unchanged files
    let mut existing_mtimes = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT path, mtime FROM files")?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
        for r in rows { if let Ok((p, m)) = r { existing_mtimes.insert(p, m); } }
    }

    conn.execute("PRAGMA foreign_keys = OFF", [])?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    
    // Structure tables only - Content tables (files, classes) are preserved for incremental speed
    let tables = ["components", "modules"];
    for table in tables { let _ = tx.execute(&format!("DELETE FROM {}", table), []); }
    
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
        let root_str = normalize_path(&def.root);
        tx.execute("INSERT OR REPLACE INTO modules (name, type, scope, root_path, build_cs_path, owner_name, component_name, deep_dependencies) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![def.name, def.mod_type, "Individual", root_str, normalize_path(&def.path), def.owner_name, def.component_name, deep_deps_json],
        )?;
        // Get the ID (either new or existing)
        let id: i64 = tx.query_row("SELECT id FROM modules WHERE name = ? AND root_path = ?", params![def.name, root_str], |r| r.get(0))?;
        mod_id_map.insert(root_str, id);
    }
    let global_mod_id = {
        tx.execute("INSERT OR REPLACE INTO modules (name, type, scope, root_path) VALUES (?, ?, ?, ?)", params!["_Global", "Global", "Game", normalize_path(&project_root)])?;
        tx.query_row("SELECT id FROM modules WHERE name = ? AND root_path = ?", params!["_Global", normalize_path(&project_root)], |r| r.get(0))?
    };
    tx.commit()?;

    // 5. Filter files for parsing
    let mut sorted_roots: Vec<(String, i64)> = mod_id_map.into_iter().collect();
    sorted_roots.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    // Update existing files' module_id in bulk if they are unchanged but point to wrong module_id
    {
        reporter.report("db_sync", 20, 100, "Verifying file-module associations...");
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare("UPDATE files SET module_id = ? WHERE path = ? AND (module_id != ? OR module_id IS NULL)")?;
            for (path_str, _) in &all_discovered_files {
                let mod_id = sorted_roots.iter().find(|(r, _)| path_str.starts_with(r)).map(|(_, id)| *id).unwrap_or(global_mod_id);
                stmt.execute(params![mod_id, path_str, mod_id])?;
            }
        } // stmt is dropped here
        tx.commit()?;
    }

    let mut headers_to_parse = Vec::new();
    let mut other_files = Vec::new();
    let mut current_on_disk = HashSet::new();

    for (path_str, ext) in all_discovered_files {
        current_on_disk.insert(path_str.clone());
        let mod_id = sorted_roots.iter().find(|(r, _)| path_str.starts_with(r)).map(|(_, id)| *id).unwrap_or(global_mod_id);
        let mtime = fs::metadata(&path_str).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0) as i64;
        
        let unchanged = existing_mtimes.get(&path_str).map_or(false, |&old| old == mtime);
        if unchanged { continue; }

        if ext == "h" || ext == "hpp" {
            headers_to_parse.push(InputFile { path: path_str, mtime: mtime as u64, old_hash: None, module_id: Some(mod_id), db_path: None });
        } else {
            other_files.push((path_str, mtime, mod_id, ext));
        }
    }

    // Remove files from DB that are no longer on disk
    {
        let tx = conn.transaction()?;
        let mut count = 0;
        for (path, _) in &existing_mtimes {
            if !current_on_disk.contains(path) {
                tx.execute("DELETE FROM files WHERE path = ?", params![path])?;
                count += 1;
            }
        }
        tx.commit()?;
        if count > 0 { tracing::info!("Cleaned up {} stale files from DB", count); }
    }

    // 6. Parallel Analysis (Only for changed headers)
    let total_headers = headers_to_parse.len();
    if total_headers > 0 {
        reporter.report("analysis", 0, total_headers, &format!("Analyzing {} changed headers...", total_headers));
        let language = tree_sitter_unreal_cpp::LANGUAGE.into();
        let query = Arc::new(Query::new(&language, scanner::QUERY_STR).expect("Failed to parse query"));
        let processed_count = Arc::new(AtomicUsize::new(0));

        let results: Vec<ParseResult> = headers_to_parse.into_par_iter().map(|input| {
            let res = scanner::process_file(&input, &language, &query).unwrap_or_else(|_| ParseResult { path: input.path, status: "error".to_string(), mtime: input.mtime, data: None, module_id: input.module_id });
            let current = processed_count.fetch_add(1, Ordering::Relaxed) + 1;
            if current % 20 == 0 || current == total_headers { 
                reporter.report("analysis", current, total_headers, &format!("Analyzing: {}/{}", current, total_headers)); 
            }
            res
        }).collect();

        // 7. Final DB Sync (Save changed headers)
        reporter.report("db_sync", 80, 100, "Saving changed results...");
        db::save_to_db(&mut conn, &results, Arc::clone(&reporter))?;
    } else {
        reporter.report("analysis", 100, 100, "No headers changed.");
    }

    // Save other files (config, shaders, etc.) that changed
    if !other_files.is_empty() {
        let tx = conn.transaction()?;
        for (path, mtime, mod_id, ext) in other_files {
            let filename = Path::new(&path).file_name().and_then(|s| s.to_str()).unwrap_or("unknown");
            tx.execute("INSERT OR REPLACE INTO files (path, filename, extension, mtime, module_id, is_header) VALUES (?, ?, ?, ?, ?, 0)", params![path, filename, ext, mtime as i64, mod_id])?;
        }
        tx.commit()?;
    }

    reporter.report("complete", 100, 100, "Refresh complete.");
    Ok(())
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace(char::from(92), "/")
}

fn get_name_from_root(path: &Path) -> String {
    path.file_name().and_then(|s| s.to_str()).unwrap_or("Unknown").to_string()
}

fn parse_build_cs(path: &Path) -> (Vec<String>, Vec<String>) {
    let content = fs::read_to_string(path).unwrap_or_default();
    let mut public_deps = Vec::new();
    let mut private_deps = Vec::new();
    let re_add_range = Regex::new(r"(Public|Private)DependencyModuleNames[.]AddRange[ \t]*[(][ \t]*new[ \t]+string[ \t]*[]][ \t]*[{](.*?)[}][ \t]*[)]").unwrap();
    let re_add = Regex::new("(Public|Private)DependencyModuleNames[.]Add[ \t]*[(][ \t]*\"(.*?)\" [ \t]*[)]").unwrap();
    let re_quoted = Regex::new("\"(.*?)\" ").unwrap();
    for cap in re_add_range.captures_iter(&content) {
        let list_type = &cap[1];
        for name_cap in re_quoted.captures_iter(&cap[2]) {
            if list_type == "Public" { public_deps.push(name_cap[1].to_string()); } else { private_deps.push(name_cap[1].to_string()); }
        }
    }
    for cap in re_add.captures_iter(&content) {
        if &cap[1] == "Public" { public_deps.push(cap[2].to_string()); } else { private_deps.push(cap[2].to_string()); }
    }
    (public_deps, private_deps)
}
