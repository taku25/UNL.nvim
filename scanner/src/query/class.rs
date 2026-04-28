use rusqlite::{Connection, params, ToSql};
use serde_json::{json, Value};
use std::collections::HashMap;
use crate::db::path::{PATH_CTE};

/// DB 内の全クラスを返す。`extra_where` は `WHERE 1=1` の後に追加するオプション句。
/// `params` は `extra_where` 内のプレースホルダーに対応するバインド値。
pub fn get_classes(
    conn: &Connection,
    extra_where: Option<&str>,
    params: &[String],
) -> anyhow::Result<Value> {
    let where_clause = extra_where.unwrap_or("");
    let sql = format!(
        "{} SELECT sc.text, sb.text, dp.full_path || '/' || sf.text, c.line_number, c.symbol_type
         FROM classes c
         JOIN strings sc ON c.name_id = sc.id
         LEFT JOIN strings sb ON c.base_class_id = sb.id
         JOIN files f ON c.file_id = f.id
         JOIN dir_paths dp ON f.directory_id = dp.id
         JOIN strings sf ON f.filename_id = sf.id
         WHERE 1=1 {}
         ORDER BY sc.text",
        PATH_CTE, where_clause
    );
    let dyn_params: Vec<&dyn ToSql> = params.iter().map(|s| s as &dyn ToSql).collect();
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(rusqlite::params_from_iter(dyn_params))?;
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        results.push(json!({
            "name": row.get::<_, String>(0)?,
            "base": row.get::<_, Option<String>>(1)?,
            "path": row.get::<_, String>(2)?,
            "line": row.get::<_, i64>(3)?,
            "type": row.get::<_, String>(4)?,
        }));
    }
    Ok(json!(results))
}

pub fn get_file_symbols(conn: &Connection, file_path: &str) -> anyhow::Result<Value> {
    // フルパスで file_id を特定する
    let file_id_sql = format!(
        "{} SELECT f.id FROM files f
         JOIN dir_paths dp ON f.directory_id = dp.id
         JOIN strings sf ON f.filename_id = sf.id
         WHERE dp.full_path || '/' || sf.text = ?
         LIMIT 1",
        PATH_CTE
    );
    let file_id: i64 = conn
        .query_row(&file_id_sql, [file_path], |r| r.get(0))
        .unwrap_or(0);

    // フルパスでマッチしなければファイル名のみでフォールバック
    let file_id = if file_id == 0 {
        let filename = std::path::Path::new(file_path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        conn.query_row(
            "SELECT id FROM files WHERE filename_id = (SELECT id FROM strings WHERE text = ? LIMIT 1) LIMIT 1",
            [filename],
            |r| r.get(0),
        )
        .unwrap_or(0)
    } else {
        file_id
    };

    if file_id == 0 {
        return Ok(json!([]));
    }

    let mut stmt = conn.prepare("
        SELECT c.id, sc.text as name, c.line_number, c.symbol_type, c.end_line_number
        FROM classes c
        JOIN strings sc ON c.name_id = sc.id
        WHERE c.file_id = ?
    ")?;

    // メンバーのファイルパスも含めて返す
    let member_sql = format!(
        "{} SELECT sn.text, st.text, m.access, m.flags, m.line_number, m.detail,
                srt.text, m.is_static,
                COALESCE(dp.full_path || '/' || sf.text, '') as file_path
         FROM members m
         JOIN strings sn  ON m.name_id = sn.id
         JOIN strings st  ON m.type_id = st.id
         LEFT JOIN strings srt ON m.return_type_id = srt.id
         LEFT JOIN files mf    ON m.file_id = mf.id
         LEFT JOIN dir_paths dp ON mf.directory_id = dp.id
         LEFT JOIN strings sf  ON mf.filename_id = sf.id
         WHERE m.class_id = ?
         ORDER BY m.line_number",
        PATH_CTE
    );

    let mut rows = stmt.query([file_id])?;
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        let class_id: i64 = row.get(0)?;
        let name: String = row.get(1)?;

        let mut m_stmt = conn.prepare(&member_sql)?;
        let mut m_rows = m_stmt.query([class_id])?;
        let mut members = Vec::new();
        while let Some(mr) = m_rows.next()? {
            let mfp: String = mr.get(8)?;
            members.push(json!({
                "name":        mr.get::<_, String>(0)?,
                "type":        mr.get::<_, String>(1)?,
                "access":      mr.get::<_, String>(2)?,
                "flags":       mr.get::<_, String>(3)?,
                "line":        mr.get::<_, i64>(4)?,
                "detail":      mr.get::<_, Option<String>>(5)?,
                "return_type": mr.get::<_, Option<String>>(6)?,
                "is_static":   mr.get::<_, i64>(7)? == 1,
                "file_path":   if mfp.is_empty() { file_path.to_string() } else { mfp },
            }));
        }

        results.push(json!({
            "name":      name,
            "line":      row.get::<_, i64>(2)?,
            "kind":      row.get::<_, String>(3)?,
            "end_line":  row.get::<_, i64>(4)?,
            "file_path": file_path,
            "members":   members,
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

/// `EHostType::Type` や `ELoadingPhase::Type` のような enum の値一覧を返す。
/// DBには name = "EHostType::Type" (フルネーム) として保存されているので、そのまま検索する。
/// フォールバックとして namespace なし ("EHostType") でも検索する。
pub fn get_enum_values(conn: &Connection, enum_name: &str) -> anyhow::Result<Value> {
    // まず渡された文字列をそのまま name として検索（"EHostType::Type" など）
    let class_id: i64 = conn.query_row(
        "SELECT c.id FROM classes c
         JOIN strings sn ON c.name_id = sn.id
         WHERE sn.text = ?
         LIMIT 1",
        params![enum_name],
        |r| r.get(0),
    ).unwrap_or(0);

    // 見つからなかった場合、"::" より後ろだけでも試みる（"Type" など）
    let class_id = if class_id == 0 {
        if let Some(pos) = enum_name.rfind("::") {
            let short_name = &enum_name[pos + 2..];
            conn.query_row(
                "SELECT c.id FROM classes c
                 JOIN strings sn ON c.name_id = sn.id
                 WHERE sn.text = ?
                 LIMIT 1",
                params![short_name],
                |r| r.get(0),
            ).unwrap_or(0)
        } else {
            0
        }
    } else {
        class_id
    };

    if class_id == 0 {
        return Ok(json!([]));
    }

    let mut stmt = conn.prepare(
        "SELECT s.text FROM enum_values ev
         JOIN strings s ON ev.name_id = s.id
         WHERE ev.enum_id = ?
         ORDER BY ev.line_number ASC"
    )?;
    let mut rows = stmt.query(params![class_id])?;
    let mut results: Vec<Value> = Vec::new();
    while let Some(row) = rows.next()? {
        results.push(json!(row.get::<_, String>(0)?));
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
