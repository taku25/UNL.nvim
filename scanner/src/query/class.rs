use rusqlite::{Connection, params, ToSql};
use serde_json::{json, Value};
use std::collections::HashMap;
use crate::db::path::{PATH_CTE};

pub fn get_file_symbols(conn: &Connection, file_path: &str) -> anyhow::Result<Value> {
    // ファイル名から ID を特定する (簡易版)
    let filename = std::path::Path::new(file_path).file_name().and_then(|s| s.to_str()).unwrap_or("");
    let file_id: i64 = conn.query_row(
        "SELECT id FROM files WHERE filename_id = (SELECT id FROM strings WHERE text = ? LIMIT 1) LIMIT 1",
        [filename], |r| r.get(0)
    ).unwrap_or(0);

    if file_id == 0 { return Ok(json!([])); }

    let mut stmt = conn.prepare("
        SELECT c.id, sc.text as name, c.line_number, c.symbol_type, c.end_line_number
        FROM classes c
        JOIN strings sc ON c.name_id = sc.id
        WHERE c.file_id = ?
    ")?;
    
    let mut rows = stmt.query([file_id])?;
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        let class_id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        
        // Members
        let mut m_stmt = conn.prepare("
            SELECT m.name_id, sn.text as name, st.text as type, m.access, m.flags, m.line_number, m.detail, srt.text as return_type, m.is_static
            FROM members m
            JOIN strings sn ON m.name_id = sn.id
            JOIN strings st ON m.type_id = st.id
            LEFT JOIN strings srt ON m.return_type_id = srt.id
            WHERE m.class_id = ?
        ")?;
        let mut m_rows = m_stmt.query([class_id])?;
        let mut members = Vec::new();
        while let Some(mr) = m_rows.next()? {
            members.push(json!({
                "name": mr.get::<_, String>(1)?,
                "type": mr.get::<_, String>(2)?,
                "access": mr.get::<_, String>(3)?,
                "flags": mr.get::<_, String>(4)?,
                "line": mr.get::<_, i64>(5)?,
                "detail": mr.get::<_, Option<String>>(6)?,
                "return_type": mr.get::<_, Option<String>>(7)?,
                "is_static": mr.get::<_, i64>(8)? == 1,
            }));
        }

        results.push(json!({
            "name": name,
            "line": row.get::<_, i64>(2)?,
            "type": row.get::<_, String>(3)?,
            "end_line": row.get::<_, i64>(4)?,
            "members": members,
        }));
    }
    Ok(json!(results))
}

pub fn get_class_members(conn: &Connection, class_name: &str) -> anyhow::Result<Value> {
    let mut stmt = conn.prepare("
        SELECT m.name_id, sn.text as name, st.text as type, m.access, m.flags, m.line_number, m.detail, srt.text as return_type, m.is_static
        FROM members m
        JOIN strings sn ON m.name_id = sn.id
        JOIN strings st ON m.type_id = st.id
        LEFT JOIN strings srt ON m.return_type_id = srt.id
        JOIN classes c ON m.class_id = c.id
        JOIN strings sc ON c.name_id = sc.id
        WHERE sc.text = ?
    ")?;
    let mut rows = stmt.query([class_name])?;
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        results.push(json!({
            "name": row.get::<_, String>(1)?,
            "type": row.get::<_, String>(2)?,
            "access": row.get::<_, String>(3)?,
            "flags": row.get::<_, String>(4)?,
            "line": row.get::<_, i64>(5)?,
            "detail": row.get::<_, Option<String>>(6)?,
            "return_type": row.get::<_, Option<String>>(7)?,
            "is_static": row.get::<_, i64>(8)? == 1,
        }));
    }
    Ok(json!(results))
}

/// モジュール群に属するクラス一覧を返す（同期版）。
/// 戻り値: [{p: path, i: [[name, line, type, base], ...]}] の grouped 形式
pub fn get_classes_in_modules(
    conn: &Connection,
    modules: Vec<String>,
    symbol_type: Option<String>,
) -> anyhow::Result<Value> {
    if modules.is_empty() {
        return Ok(json!([]));
    }

    // path → Vec<[name, line, type, base]>
    let mut grouped: HashMap<String, Vec<Value>> = HashMap::new();

    for chunk in modules.chunks(500) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let type_clause = if symbol_type.is_some() { " AND c.symbol_type = ?" } else { "" };
        let sql = format!(
            "{}
             SELECT sc.text, sb.text, dp.full_path || '/' || sf.text, c.line_number, c.symbol_type
             FROM classes c
             JOIN strings sc ON c.name_id = sc.id
             LEFT JOIN strings sb ON c.base_class_id = sb.id
             JOIN files f ON c.file_id = f.id
             JOIN dir_paths dp ON f.directory_id = dp.id
             JOIN strings sf ON f.filename_id = sf.id
             JOIN modules m ON f.module_id = m.id
             JOIN strings sm ON m.name_id = sm.id
             WHERE sm.text IN ({}){}
             ORDER BY dp.full_path || '/' || sf.text, c.line_number",
            PATH_CTE, placeholders, type_clause
        );

        let mut dyn_params: Vec<&dyn ToSql> = chunk.iter().map(|s| s as &dyn ToSql).collect();
        if let Some(ref st) = symbol_type {
            dyn_params.push(st);
        }

        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(dyn_params))?;
        while let Some(row) = rows.next()? {
            let name: String = row.get(0)?;
            let base: Option<String> = row.get(1)?;
            let path: String = row.get(2)?;
            let line: i64 = row.get(3)?;
            let sym_type: String = row.get(4)?;
            grouped.entry(path).or_default().push(json!([name, line, sym_type, base]));
        }
    }

    let result: Vec<Value> = grouped.into_iter().map(|(p, i)| json!({"p": p, "i": i})).collect();
    Ok(json!(result))
}

/// モジュール群に属するクラス一覧をストリーミング配信する（非同期版）。
/// on_items に渡す各アイテム: {name, base, path, line, type}
pub fn get_classes_in_modules_async<F>(
    conn: &Connection,
    modules: Vec<String>,
    symbol_type: Option<String>,
    mut on_items: F,
) -> anyhow::Result<Value>
where
    F: FnMut(Vec<Value>) -> anyhow::Result<()>,
{
    if modules.is_empty() {
        return Ok(json!(0));
    }

    let mut total_sent = 0usize;

    for chunk in modules.chunks(500) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let type_clause = if symbol_type.is_some() { " AND c.symbol_type = ?" } else { "" };
        let sql = format!(
            "{}
             SELECT sc.text, sb.text, dp.full_path || '/' || sf.text, c.line_number, c.symbol_type
             FROM classes c
             JOIN strings sc ON c.name_id = sc.id
             LEFT JOIN strings sb ON c.base_class_id = sb.id
             JOIN files f ON c.file_id = f.id
             JOIN dir_paths dp ON f.directory_id = dp.id
             JOIN strings sf ON f.filename_id = sf.id
             JOIN modules m ON f.module_id = m.id
             JOIN strings sm ON m.name_id = sm.id
             WHERE sm.text IN ({}){}",
            PATH_CTE, placeholders, type_clause
        );

        let mut dyn_params: Vec<&dyn ToSql> = chunk.iter().map(|s| s as &dyn ToSql).collect();
        if let Some(ref st) = symbol_type {
            dyn_params.push(st);
        }

        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(dyn_params))?;
        let mut batch: Vec<Value> = Vec::new();
        while let Some(row) = rows.next()? {
            let name: String = row.get(0)?;
            let base: Option<String> = row.get(1)?;
            let path: String = row.get(2)?;
            let line: i64 = row.get(3)?;
            let sym_type: String = row.get(4)?;
            batch.push(json!({"name": name, "base": base, "path": path, "line": line, "type": sym_type}));
            if batch.len() >= 200 {
                total_sent += batch.len();
                on_items(std::mem::take(&mut batch))?;
            }
        }
        if !batch.is_empty() {
            total_sent += batch.len();
            on_items(std::mem::take(&mut batch))?;
        }
    }

    Ok(json!(total_sent))
}

pub fn find_symbol_usages(conn: &Connection, symbol_name: &str, limit: usize) -> anyhow::Result<Value> {
    let sql = format!("
        {}
        SELECT sc.line, dp.full_path || '/' || sn.text as path
        FROM symbol_calls sc
        JOIN strings s ON sc.name_id = s.id
        JOIN files f ON sc.file_id = f.id
        JOIN dir_paths dp ON f.directory_id = dp.id
        JOIN strings sn ON f.filename_id = sn.id
        WHERE s.text = ?
        LIMIT ?
    ", PATH_CTE);
    
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![symbol_name, limit])?;
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        results.push(json!({
            "line": row.get::<_, i64>(0)?,
            "path": row.get::<_, String>(1)?,
        }));
    }
    Ok(json!(results))
}
