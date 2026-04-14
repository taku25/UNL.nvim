use rusqlite::{Connection};
use serde_json::{json, Value};
use crate::db::path::{PATH_CTE};

/// ファイルの依存関係を取得する
pub fn get_depend_files(conn: &Connection, file_path: &str, recursive: bool, game_only: bool) -> anyhow::Result<Value> {
    let mut results = Vec::new();
    
    let file_id: i64 = match conn.query_row(
        "SELECT id FROM files WHERE filename_id = (SELECT id FROM strings WHERE text = ? LIMIT 1) LIMIT 1",
        [std::path::Path::new(file_path).file_name().and_then(|s| s.to_str()).unwrap_or("")],
        |row| row.get(0)
    ) {
        Ok(id) => id,
        Err(_) => return Ok(json!(results)),
    };

    let sql = if recursive {
        format!("
            {}
            WITH RECURSIVE dependency_graph(file_id, resolved_id) AS (
                SELECT file_id, resolved_file_id FROM file_includes WHERE file_id = ?
                UNION
                SELECT fi.file_id, fi.resolved_file_id 
                FROM file_includes fi 
                JOIN dependency_graph dg ON fi.file_id = dg.resolved_id 
                WHERE fi.resolved_file_id IS NOT NULL
            )
            SELECT DISTINCT 
                dp.full_path || '/' || sn.text as path, 
                sm.text as module_name, 
                rd.full_path as module_root, 
                f.extension
            FROM dependency_graph dg 
            JOIN files f ON dg.resolved_id = f.id
            JOIN dir_paths dp ON f.directory_id = dp.id
            JOIN strings sn ON f.filename_id = sn.id
            LEFT JOIN modules m ON f.module_id = m.id
            LEFT JOIN strings sm ON m.name_id = sm.id
            LEFT JOIN dir_paths rd ON m.root_directory_id = rd.id
        ", PATH_CTE)
    } else {
        format!("
            {}
            SELECT DISTINCT 
                dp.full_path || '/' || sn.text as path, 
                sm.text as module_name, 
                rd.full_path as module_root, 
                f.extension
            FROM file_includes fi 
            JOIN files f ON fi.resolved_file_id = f.id
            JOIN dir_paths dp ON f.directory_id = dp.id
            JOIN strings sn ON f.filename_id = sn.id
            LEFT JOIN modules m ON f.module_id = m.id
            LEFT JOIN strings sm ON m.name_id = sm.id
            LEFT JOIN dir_paths rd ON m.root_directory_id = rd.id
            WHERE fi.file_id = ? AND fi.resolved_file_id IS NOT NULL
        ", PATH_CTE)
    };

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([file_id])?;
    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        if !game_only || (!path.contains("/Engine/") && !path.contains("\\Engine\\")) {
            results.push(json!({
                "file_path": path,
                "module_name": row.get::<_, Option<String>>(1)?,
                "module_root": row.get::<_, Option<String>>(2)?,
                "extension": row.get::<_, String>(3)?
            }));
        }
    }
    Ok(json!(results))
}

/// モジュール内の全ファイルを取得する
pub fn get_files_in_modules(conn: &Connection, modules: Vec<String>, extensions: Option<Vec<String>>, filter: Option<String>) -> anyhow::Result<Value> {
    let sql = format!("
        {}
        SELECT dp.full_path || '/' || sn.text as path, sm.text as module_name, rd.full_path as module_root, f.extension
        FROM files f
        JOIN dir_paths dp ON f.directory_id = dp.id
        JOIN strings sn ON f.filename_id = sn.id
        JOIN modules m ON f.module_id = m.id
        JOIN strings sm ON m.name_id = sm.id
        JOIN dir_paths rd ON m.root_directory_id = rd.id
        WHERE sm.text IN ({})
    ", PATH_CTE, modules.iter().map(|_| "?").collect::<Vec<_>>().join(","));

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(rusqlite::params_from_iter(modules))?;
    
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        let ext: String = row.get(3)?;
        
        let mut match_filter = true;
        if let Some(ref exts) = extensions {
            if !exts.contains(&ext) { match_filter = false; }
        }
        if match_filter {
            if let Some(ref f) = filter {
                if !path.contains(f) { match_filter = false; }
            }
        }

        if match_filter {
            results.push(json!({
                "file_path": path,
                "module_name": row.get::<_, String>(1)?,
                "module_root": row.get::<_, String>(2)?,
                "extension": ext
            }));
        }
    }
    Ok(json!(results))
}

/// パスの一部（部分文字列）でファイルを検索する。`#include` ナビゲーション等で使用。
pub fn search_files_by_path_part(conn: &Connection, part: &str) -> anyhow::Result<Value> {
    let sql = format!("
        {}
        SELECT dp.full_path || '/' || sn.text as path,
               sm.text as module_name,
               rd.full_path as module_root,
               f.extension
        FROM files f
        JOIN dir_paths dp ON f.directory_id = dp.id
        JOIN strings sn ON f.filename_id = sn.id
        LEFT JOIN modules m ON f.module_id = m.id
        LEFT JOIN strings sm ON m.name_id = sm.id
        LEFT JOIN dir_paths rd ON m.root_directory_id = rd.id
        WHERE (dp.full_path || '/' || sn.text) LIKE ?
        LIMIT 100
    ", PATH_CTE);

    let pattern = format!("%{}%", part);
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([pattern])?;

    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        results.push(json!({
            "path": row.get::<_, String>(0)?,
            "module_name": row.get::<_, Option<String>>(1)?,
            "module_root": row.get::<_, Option<String>>(2)?,
            "extension": row.get::<_, String>(3)?,
        }));
    }
    Ok(json!(results))
}

pub fn get_files_in_modules_async<F>(conn: &Connection, modules: Vec<String>, extensions: Option<Vec<String>>, filter: Option<String>, mut on_items: F) -> anyhow::Result<Value>
where F: FnMut(Vec<Value>) -> anyhow::Result<()> {
    let sql = format!("
        {}
        SELECT dp.full_path || '/' || sn.text as path, sm.text as module_name, rd.full_path as module_root, f.extension
        FROM files f
        JOIN dir_paths dp ON f.directory_id = dp.id
        JOIN strings sn ON f.filename_id = sn.id
        JOIN modules m ON f.module_id = m.id
        JOIN strings sm ON m.name_id = sm.id
        JOIN dir_paths rd ON m.root_directory_id = rd.id
        WHERE sm.text IN ({})
    ", PATH_CTE, modules.iter().map(|_| "?").collect::<Vec<_>>().join(","));

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(rusqlite::params_from_iter(modules))?;
    
    let mut batch = Vec::new();
    let mut total_count = 0;

    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        let ext: String = row.get(3)?;
        
        let mut match_filter = true;
        if let Some(ref exts) = extensions {
            if !exts.contains(&ext) { match_filter = false; }
        }
        if match_filter {
            if let Some(ref f) = filter {
                if !path.contains(f) { match_filter = false; }
            }
        }

        if match_filter {
            batch.push(json!({
                "file_path": path,
                "module_name": row.get::<_, String>(1)?,
                "module_root": row.get::<_, String>(2)?,
                "extension": ext
            }));
            total_count += 1;

            if batch.len() >= 500 {
                on_items(std::mem::take(&mut batch))?;
            }
        }
    }

    if !batch.is_empty() {
        on_items(batch)?;
    }

    Ok(json!(total_count))
}

/// *.Target.cs ファイルの一覧を取得する
pub fn get_target_files(conn: &Connection) -> anyhow::Result<Value> {
    let sql = format!("
        {}
        SELECT sn.text as filename, dp.full_path || '/' || sn.text as path
        FROM files f
        JOIN dir_paths dp ON f.directory_id = dp.id
        JOIN strings sn ON f.filename_id = sn.id
        WHERE sn.text LIKE '%.Target.cs'
    ", PATH_CTE);
    
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        results.push(json!({
            "filename": row.get::<_, String>(0)?,
            "path": row.get::<_, String>(1)?,
        }));
    }
    Ok(json!(results))
}

/// 全てのファイルパスをリストで取得する
pub fn get_all_file_paths(conn: &Connection) -> anyhow::Result<Value> {
    let sql = format!("
        {}
        SELECT dp.full_path || '/' || sn.text as path
        FROM files f
        JOIN dir_paths dp ON f.directory_id = dp.id
        JOIN strings sn ON f.filename_id = sn.id
    ", PATH_CTE);
    
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        results.push(Value::String(row.get(0)?));
    }
    Ok(json!(results))
}

/// 全てのファイルのメタデータ (filename, path, module_name) を取得する
pub fn get_all_files_metadata(conn: &Connection) -> anyhow::Result<Value> {
    let sql = format!("
        {}
        SELECT sn.text as filename, dp.full_path || '/' || sn.text as path, sm.text as module_name
        FROM files f
        JOIN dir_paths dp ON f.directory_id = dp.id
        JOIN strings sn ON f.filename_id = sn.id
        LEFT JOIN modules m ON f.module_id = m.id
        LEFT JOIN strings sm ON m.name_id = sm.id
    ", PATH_CTE);
    
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        results.push(json!({
            "filename": row.get::<_, String>(0)?,
            "path": row.get::<_, String>(1)?,
            "module_name": row.get::<_, Option<String>>(2)?,
        }));
    }
    Ok(json!(results))
}
