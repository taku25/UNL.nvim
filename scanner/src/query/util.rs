use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use crate::db::path::{PATH_CTE};

/// クラス名からその定義ファイルのフルパスを取得する
pub fn get_class_file_path(conn: &Connection, class_name: &str) -> anyhow::Result<Value> {
    let sql = format!("
        {}
        SELECT dp.full_path || '/' || sn.text
        FROM classes c
        JOIN strings sc ON c.name_id = sc.id
        JOIN files f ON c.file_id = f.id
        JOIN dir_paths dp ON f.directory_id = dp.id
        JOIN strings sn ON f.filename_id = sn.text
        WHERE sc.text = ? LIMIT 1
    ", PATH_CTE);

    let mut stmt = conn.prepare(&sql)?;
    let res = stmt.query_row([class_name], |row| Ok(row.get::<_, String>(0)?)).optional()?;
    Ok(json!(res))
}
