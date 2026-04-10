use rusqlite::{Connection, params};
use serde_json::{json, Value};
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
