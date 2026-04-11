use std::path::{Path, Component};
use std::collections::HashMap;
use rusqlite::{params, Connection, Transaction};

/// ディレクトリツリーを辿って directory_id を取得または作成する
pub fn get_or_create_directory(
    tx: &Transaction, 
    str_cache: &mut HashMap<String, i64>,
    dir_cache: &mut HashMap<(Option<i64>, i64), i64>,
    path: &Path
) -> rusqlite::Result<i64> {
    let mut current_parent_id: Option<i64> = None;

    for component in path.components() {
        let name = match component {
            Component::Normal(s) => s.to_string_lossy().to_string(),
            Component::RootDir => "/".to_string(),
            Component::Prefix(p) => p.as_os_str().to_string_lossy().to_string(),
            _ => continue,
        };

        let name_id = crate::db::get_or_create_string(tx, str_cache, &name)?;
        let cache_key = (current_parent_id, name_id);

        if let Some(&id) = dir_cache.get(&cache_key) {
            current_parent_id = Some(id);
        } else {
            let id: i64 = match tx.query_row(
                "SELECT id FROM directories WHERE (parent_id IS ? OR parent_id = ?) AND name_id = ?",
                params![current_parent_id, current_parent_id, name_id],
                |row| row.get(0)
            ) {
                Ok(id) => id,
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    tx.execute(
                        "INSERT INTO directories (parent_id, name_id) VALUES (?, ?)",
                        params![current_parent_id, name_id]
                    )?;
                    tx.last_insert_rowid()
                },
                Err(e) => return Err(e),
            };
            dir_cache.insert(cache_key, id);
            current_parent_id = Some(id);
        }
    }

    Ok(current_parent_id.unwrap_or(0))
}

/// directory_id からフルパスを復元する (Rust側)
pub fn get_full_path(conn: &Connection, directory_id: i64, filename_id: i64) -> anyhow::Result<String> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE path_builder(id, parent_id, name_id) AS (
            SELECT id, parent_id, name_id FROM directories WHERE id = ?
            UNION ALL
            SELECT d.id, d.parent_id, d.name_id FROM directories d JOIN path_builder pb ON d.id = pb.parent_id
        )
        SELECT s.text FROM path_builder pb JOIN strings s ON pb.name_id = s.id"
    )?;
    
    let mut rows = stmt.query([directory_id])?;
    let mut parts = Vec::new();
    while let Some(row) = rows.next()? {
        parts.push(row.get::<_, String>(0)?);
    }
    parts.reverse();
    
    let filename: String = conn.query_row("SELECT text FROM strings WHERE id = ?", [filename_id], |r| r.get(0))?;
    
    let mut full = parts.join("/");
    if !full.ends_with('/') && !full.is_empty() { full.push('/'); }
    full.push_str(&filename);
    Ok(full.replace("//", "/").replace("\\", "/"))
}

/// SQL クエリ内でフルパスを生成するための共通 CTE 文字列
pub const PATH_CTE: &str = "
    WITH RECURSIVE dir_paths(id, full_path) AS (
        SELECT d.id, s.text FROM directories d JOIN strings s ON d.name_id = s.id WHERE d.parent_id IS NULL
        UNION ALL
        SELECT d.id, CASE WHEN dp.full_path = '/' THEN '/' || s.text ELSE dp.full_path || '/' || s.text END
        FROM directories d
        JOIN dir_paths dp ON d.parent_id = dp.id
        JOIN strings s ON d.name_id = s.id
    )
";

/// files テーブルと dir_paths を結合してパスを取得する共通 SELECT 部分
pub const FILE_PATH_SELECT: &str = "
    SELECT f.*, dp.full_path || '/' || sn.text as path
    FROM files f
    JOIN dir_paths dp ON f.directory_id = dp.id
    JOIN strings sn ON f.filename_id = sn.id
";
