use rusqlite::{Connection, ToSql};
use serde_json::{json, Value};
use rayon::prelude::*;
use memmap2::Mmap;
use std::fs::File;

pub fn grep_assets<F>(conn: &Connection, pattern: String, mut on_items: F) -> anyhow::Result<Value> 
where F: FnMut(Vec<Value>) -> anyhow::Result<()> {
    tracing::info!("Grepping assets for pattern: '{}'", pattern);
    
    let mut stmt = conn.prepare(
        "SELECT path FROM files 
         WHERE (LOWER(extension) = 'uasset' OR LOWER(extension) = 'umap') 
         AND path LIKE '%/Content/%'"
    )?;
    
    let file_paths: Vec<String> = stmt.query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    
    let file_count = file_paths.len();
    tracing::info!("Scanning {} asset files using mmap...", file_count);

    let pattern_bytes = pattern.as_bytes();
    
    let results: Vec<String> = file_paths.par_iter()
        .filter(|path| {
            match File::open(path) {
                Ok(file) => {
                    unsafe {
                        match Mmap::map(&file) {
                            Ok(mmap) => {
                                mmap.windows(pattern_bytes.len()).any(|window| window == pattern_bytes)
                            },
                            Err(_) => false
                        }
                    }
                },
                Err(_) => false,
            }
        })
        .cloned()
        .collect();

    tracing::info!("Grep finished. Found {} matches in {} files.", results.len(), file_count);
    
    for chunk in results.chunks(500) {
        let items: Vec<Value> = chunk.iter().map(|p| json!(p)).collect();
        on_items(items)?;
    }
    
    Ok(json!(results.len()))
}

pub fn search_files(conn: &Connection, part: String) -> anyhow::Result<Value> {
    let mut stmt = conn.prepare(
        "SELECT path, filename FROM files WHERE filename LIKE ? LIMIT 100"
    )?;
    let param = format!("%{}%", part);
    let rows = stmt.query_map([param], |row| {
        Ok(json!({
            "path": row.get::<_, String>(0)?,
            "filename": row.get::<_, String>(1)?,
        }))
    })?;
    let res: Result<Vec<Value>, _> = rows.collect();
    Ok(json!(res?))
}

pub fn search_files_in_modules(conn: &Connection, modules: Vec<String>, filter: String, limit: Option<usize>) -> anyhow::Result<Value> {
     if modules.is_empty() { return Ok(json!([])); }
     let limit_val = limit.unwrap_or(100);
     let mut all_files = Vec::new();
     for chunk in modules.chunks(500) {
         if all_files.len() >= limit_val { break; }
         let remaining = limit_val - all_files.len();
         let placeholders: Vec<String> = chunk.iter().map(|_| "?".to_string()).collect();
         let sql = format!("SELECT f.path, f.extension, m.name, m.root_path FROM files f JOIN modules m ON f.module_id = m.id WHERE m.name IN ({}) AND f.path LIKE ? LIMIT ?", placeholders.join(","));
         let filter_param = format!("%{}%", filter);
         let mut params: Vec<&dyn ToSql> = chunk.iter().map(|s| s as &dyn ToSql).collect();
         params.push(&filter_param);
         let limit_param = remaining as i64;
         params.push(&limit_param);
         let mut stmt = conn.prepare(&sql)?;
         let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| Ok(json!({ "file_path": row.get::<_, String>(0)?, "extension": row.get::<_, String>(1)?, "module_name": row.get::<_, String>(2)?, "module_root": row.get::<_, String>(3)? })))?;
         for r in rows { all_files.push(r?); }
     }
     Ok(json!(all_files))
}

pub fn search_files_by_path_part(conn: &Connection, part: String) -> anyhow::Result<Value> {
    let mut stmt = conn.prepare("SELECT f.path, f.filename, m.root_path FROM files f JOIN modules m ON f.module_id = m.id WHERE f.path LIKE ? LIMIT 50")?;
    let param = format!("%{}%", part);
    let rows = stmt.query_map([param], |row| Ok(json!({ "path": row.get::<_, String>(0)?, "filename": row.get::<_, String>(1)?, "module_root": row.get::<_, String>(2)? })))?;
    Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
}