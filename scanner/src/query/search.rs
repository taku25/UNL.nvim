use rusqlite::{Connection, params};
use serde_json::{json, Value};
use crate::db::path::{PATH_CTE};

/// シンボル名で FTS 検索を行い、パス付きで結果を返す
pub fn search_symbols(conn: &Connection, pattern: &str, limit: usize) -> anyhow::Result<Value> {
    let sql = format!("
        {}
        SELECT sfts.name, sfts.type, sfts.class_name, dp.full_path || '/' || sn.text as path
        FROM symbols_fts sfts
        JOIN classes c ON sfts.rowid_ref = c.id
        JOIN files f ON c.file_id = f.id
        JOIN dir_paths dp ON f.directory_id = dp.id
        JOIN strings sn ON f.filename_id = sn.id
        WHERE symbols_fts MATCH ?
        LIMIT ?
    ", PATH_CTE);

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![pattern, limit])?;
    
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        results.push(json!({
            "name": row.get::<_, String>(0)?,
            "type": row.get::<_, String>(1)?,
            "class_name": row.get::<_, String>(2)?,
            "path": row.get::<_, String>(3)?,
        }));
    }
    Ok(json!(results))
}

/// 全ての構造体を取得する (USX 用など)
pub fn get_structs(conn: &Connection) -> anyhow::Result<Value> {
    let sql = format!("
        {}
        SELECT sc.text as name, sb.text as base_class, c.symbol_type, dp.full_path || '/' || sn.text as path, sm.text as module_name
        FROM classes c
        JOIN strings sc ON c.name_id = sc.id
        LEFT JOIN strings sb ON c.base_class_id = sb.id
        JOIN files f ON c.file_id = f.id
        JOIN dir_paths dp ON f.directory_id = dp.id
        JOIN strings sn ON f.filename_id = sn.id
        JOIN modules m ON f.module_id = m.id
        JOIN strings sm ON m.name_id = sm.id
        WHERE c.symbol_type = 'struct' AND sc.text NOT LIKE '(%'
        ORDER BY sc.text ASC
    ", PATH_CTE);

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        results.push(json!({
            "name": row.get::<_, String>(0)?,
            "base_class": row.get::<_, Option<String>>(1)?,
            "type": row.get::<_, String>(2)?,
            "path": row.get::<_, String>(3)?,
            "module_name": row.get::<_, String>(4)?,
        }));
    }
    Ok(json!(results))
}
