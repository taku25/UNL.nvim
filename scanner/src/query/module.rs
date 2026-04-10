use rusqlite::{Connection};
use serde_json::{json, Value};
use crate::db::path::{PATH_CTE};

pub fn get_modules(conn: &Connection) -> anyhow::Result<Value> {
    let sql = format!("
        {}
        SELECT sm.text as name, m.type, m.scope, dp.full_path as root_path, m.build_cs_path, m.owner_name, m.component_name, m.deep_dependencies
        FROM modules m
        JOIN strings sm ON m.name_id = sm.id
        JOIN dir_paths dp ON m.root_directory_id = dp.id
    ", PATH_CTE);
    
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        results.push(json!({
            "name": row.get::<_, String>(0)?,
            "type": row.get::<_, String>(1)?,
            "scope": row.get::<_, String>(2)?,
            "module_root": row.get::<_, String>(3)?,
            "build_cs_path": row.get::<_, Option<String>>(4)?,
            "owner_name": row.get::<_, Option<String>>(5)?,
            "component_name": row.get::<_, Option<String>>(6)?,
            "deep_dependencies": row.get::<_, Option<String>>(7)?,
        }));
    }
    Ok(json!(results))
}

pub fn get_module_by_name(conn: &Connection, name: &str) -> anyhow::Result<Value> {
    let sql = format!("
        {}
        SELECT sm.text as name, m.type, m.scope, dp.full_path as root_path, m.id
        FROM modules m
        JOIN strings sm ON m.name_id = sm.id
        JOIN dir_paths dp ON m.root_directory_id = dp.id
        WHERE sm.text = ? LIMIT 1
    ", PATH_CTE);
    
    let res = conn.query_row(&sql, [name], |row| {
        let mid: i64 = row.get(4)?;
        Ok(json!({
            "name": row.get::<_, String>(0)?,
            "type": row.get::<_, String>(1)?,
            "scope": row.get::<_, String>(2)?,
            "module_root": row.get::<_, String>(3)?,
            "id": mid,
        }))
    })?;
    Ok(res)
}
