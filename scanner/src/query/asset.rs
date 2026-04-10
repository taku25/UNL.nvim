use rusqlite::{Connection, ToSql};
use serde_json::{json, Value};
use rayon::prelude::*;
use memmap2::Mmap;
use std::fs::File;
use crate::db::path::{PATH_CTE};

pub fn grep_assets<F>(conn: &Connection, pattern: String, mut on_items: F) -> anyhow::Result<Value> 
where F: FnMut(Vec<Value>) -> anyhow::Result<()> {
    tracing::info!("Grepping assets for pattern: '{}'", pattern);
    
    let sql = format!("
        {}
        SELECT dp.full_path || '/' || sn.text
        FROM files f 
        JOIN dir_paths dp ON f.directory_id = dp.id
        JOIN strings sn ON f.filename_id = sn.id
        WHERE (LOWER(f.extension) = 'uasset' OR LOWER(f.extension) = 'umap') 
        AND dp.full_path LIKE '%/Content/%'
    ", PATH_CTE);
    
    let mut stmt = conn.prepare(&sql)?;
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
                            Ok(mmap) => mmap.windows(pattern_bytes.len()).any(|window| window == pattern_bytes),
                            Err(_) => false
                        }
                    }
                },
                Err(_) => false,
            }
        })
        .cloned()
        .collect();

    for chunk in results.chunks(500) {
        let items: Vec<Value> = chunk.iter().map(|p| json!(p)).collect();
        on_items(items)?;
    }
    
    Ok(json!(results.len()))
}

pub fn get_assets(conn: &Connection) -> anyhow::Result<Value> {
    let sql = format!("
        {}
        SELECT dp.full_path || '/' || sn.text as path, sn.text as filename
        FROM files f
        JOIN dir_paths dp ON f.directory_id = dp.id
        JOIN strings sn ON f.filename_id = sn.id
        WHERE LOWER(f.extension) IN ('uasset', 'umap')
        LIMIT 1000
    ", PATH_CTE);
    
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(json!({
            "path": row.get::<_, String>(0)?,
            "filename": row.get::<_, String>(1)?,
        }))
    })?;
    Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
}

pub fn search_files(conn: &Connection, part: String) -> anyhow::Result<Value> {
    let sql = format!("
        {}
        SELECT dp.full_path || '/' || sn.text as path, sn.text as filename 
        FROM files f 
        JOIN dir_paths dp ON f.directory_id = dp.id
        JOIN strings sn ON f.filename_id = sn.id
        WHERE sn.text LIKE ? LIMIT 100
    ", PATH_CTE);
    let mut stmt = conn.prepare(&sql)?;
    let param = format!("%{}%", part);
    let rows = stmt.query_map([param], |row| {
        Ok(json!({
            "path": row.get::<_, String>(0)?,
            "filename": row.get::<_, String>(1)?,
        }))
    })?;
    Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
}

pub fn search_files_in_modules(conn: &Connection, modules: Vec<String>, filter: String, limit: Option<usize>) -> anyhow::Result<Value> {
     if modules.is_empty() { return Ok(json!([])); }
     let limit_val = limit.unwrap_or(100);
     let mut all_files = Vec::new();
     for chunk in modules.chunks(500) {
         if all_files.len() >= limit_val { break; }
         let remaining = limit_val - all_files.len();
         let placeholders: Vec<String> = chunk.iter().map(|_| "?".to_string()).collect();
         let sql = format!(
            "{}
             SELECT dp.full_path || '/' || sn.text, f.extension, sm.text, rd.full_path
             FROM files f 
             JOIN dir_paths dp ON f.directory_id = dp.id
             JOIN strings sn ON f.filename_id = sn.id
             JOIN modules m ON f.module_id = m.id 
             JOIN strings sm ON m.name_id = sm.id
             JOIN dir_paths rd ON m.root_directory_id = rd.id
             WHERE sm.text IN ({}) AND (dp.full_path || '/' || sn.text) LIKE ? LIMIT ?", 
            PATH_CTE, placeholders.join(",")
         );
         let filter_param = format!("%{}%", filter);
         let mut params: Vec<&dyn ToSql> = chunk.iter().map(|s| s as &dyn ToSql).collect();
         params.push(&filter_param);
         let limit_param = remaining as i64;
         params.push(&limit_param);
         let mut stmt = conn.prepare(&sql)?;
         let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
             Ok(json!({ "file_path": row.get::<_, String>(0)?, "extension": row.get::<_, String>(1)?, "module_name": row.get::<_, String>(2)?, "module_root": row.get::<_, String>(3)? }))
         })?;
         for r in rows { all_files.push(r?); }
     }
     Ok(json!(all_files))
}

pub fn search_files_in_modules_async<F>(conn: &Connection, modules: Vec<String>, filter: String, limit: Option<usize>, mut on_items: F) -> anyhow::Result<Value>
where F: FnMut(Vec<Value>) -> anyhow::Result<()> {
     if modules.is_empty() { return Ok(json!(0)); }
     let limit_val = limit.unwrap_or(1000);
     let mut total_sent = 0;
     
     for chunk in modules.chunks(500) {
         if total_sent >= limit_val { break; }
         let remaining = limit_val - total_sent;
         let placeholders: Vec<String> = chunk.iter().map(|_| "?".to_string()).collect();
         let sql = format!(
            "{}
             SELECT dp.full_path || '/' || sn.text, f.extension, sm.text, rd.full_path
             FROM files f 
             JOIN dir_paths dp ON f.directory_id = dp.id
             JOIN strings sn ON f.filename_id = sn.id
             JOIN modules m ON f.module_id = m.id 
             JOIN strings sm ON m.name_id = sm.id
             JOIN dir_paths rd ON m.root_directory_id = rd.id
             WHERE sm.text IN ({}) AND (dp.full_path || '/' || sn.text) LIKE ? LIMIT ?", 
            PATH_CTE, placeholders.join(",")
         );
         
         let filter_param = format!("%{}%", filter);
         let mut params: Vec<&dyn ToSql> = chunk.iter().map(|s| s as &dyn ToSql).collect();
         params.push(&filter_param);
         let limit_param = remaining as i64;
         params.push(&limit_param);
         
         let mut stmt = conn.prepare(&sql)?;
         let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
             Ok(json!({ "file_path": row.get::<_, String>(0)?, "extension": row.get::<_, String>(1)?, "module_name": row.get::<_, String>(2)?, "module_root": row.get::<_, String>(3)? }))
         })?;
         
         let mut batch = Vec::new();
         for r in rows {
             batch.push(r?);
             if batch.len() >= 200 {
                 total_sent += batch.len();
                 on_items(std::mem::take(&mut batch))?;
             }
         }
         if !batch.is_empty() {
             total_sent += batch.len();
             on_items(batch)?;
         }
     }
     Ok(json!(total_sent))
}
