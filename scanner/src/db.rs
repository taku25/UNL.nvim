use std::path::Path;
use std::sync::Arc;
use rusqlite::{params, Connection};
use crate::types::{ParseResult, ProgressReporter};

pub const DB_VERSION: i32 = 1;

pub fn init_db(conn: &Connection) -> rusqlite::Result<()> {
    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
    
    // ... (既存のテーブル作成ロジック) ...
    // 8. Components
    conn.execute(
        "CREATE TABLE IF NOT EXISTS components (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            display_name TEXT,
            type TEXT,
            owner_name TEXT,
            root_path TEXT,
            uplugin_path TEXT,
            uproject_path TEXT,
            engine_association TEXT
        )",
        [],
    )?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_components_type ON components(type)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_components_owner ON components(owner_name)", [])?;

    // Set DB version
    conn.execute(
        "INSERT OR REPLACE INTO project_meta (key, value) VALUES ('db_version', ?)",
        [DB_VERSION.to_string()],
    )?;

    Ok(())
}

pub fn save_to_db(conn: &mut Connection, results: &[ParseResult], reporter: Arc<dyn ProgressReporter>) -> anyhow::Result<()> {
    conn.busy_timeout(std::time::Duration::from_millis(30000))?;
    let _ = conn.pragma_update(None, "journal_mode", "WAL");
    let _ = conn.pragma_update(None, "synchronous", "OFF"); // Max speed during bulk
    let _ = conn.pragma_update(None, "cache_size", "-800000"); // 800MB cache
    let _ = conn.pragma_update(None, "temp_store", "MEMORY");
    
    conn.execute("PRAGMA foreign_keys = ON", [])?; // Must be ON for CASCADE

    let total = results.len();
    reporter.report("db_sync", 0, total, &format!("Saving to DB (0/{})", total));

    const BATCH_SIZE: usize = 2000;
    let mut current_idx = 0;

    while current_idx < total {
        let end_idx = std::cmp::min(current_idx + BATCH_SIZE, total);
        let batch = &results[current_idx..end_idx];

        let tx = conn.transaction()?;
        {
            let mut stmt_file = tx.prepare("INSERT OR REPLACE INTO files (path, filename, extension, mtime, file_hash, module_id, is_header) VALUES (?, ?, ?, ?, ?, ?, ?)")?;
            let mut stmt_class = tx.prepare("INSERT OR IGNORE INTO classes (name, namespace, base_class, file_id, line_number, symbol_type, end_line_number) VALUES (?, ?, ?, ?, ?, ?, ?)")?;
            let mut stmt_class_id = tx.prepare("SELECT id FROM classes WHERE name = ? AND file_id = ? LIMIT 1")?;
            let mut stmt_inheritance = tx.prepare("INSERT OR IGNORE INTO inheritance (child_id, parent_name) VALUES (?, ?)")?;
            let mut stmt_enum = tx.prepare("INSERT OR IGNORE INTO enum_values (enum_id, name) VALUES (?, ?)")?;
            let mut stmt_member = tx.prepare("INSERT OR IGNORE INTO members (class_id, name, type, flags, access, detail, return_type, is_static, line_number) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")?;

            for (i, result) in batch.iter().enumerate() {
                let global_i = current_idx + i;
                if global_i % 200 == 0 {
                    reporter.report("db_sync", global_i, total, &format!("Saving results ({}/{})", global_i, total));
                }
                
                if result.status != "parsed" { continue; }
                let data = match &result.data {
                    Some(d) => d,
                    None => continue,
                };

                let path_obj = Path::new(&result.path);
                let filename = path_obj.file_name().and_then(|s| s.to_str()).unwrap_or("unknown");
                let extension = path_obj.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
                
                let safe_module_id = if let Some(id) = result.module_id {
                    if id <= 0 { None } else { Some(id) }
                } else {
                    None
                };

                let file_res = stmt_file.execute(params![
                    result.path, filename, extension, result.mtime as i64, data.new_hash, safe_module_id,
                    if extension == "h" || extension == "hpp" { 1 } else { 0 }
                ]);

                if file_res.is_ok() {
                    let file_id: i64 = tx.last_insert_rowid();

                    for cls in &data.classes {
                        let _ = stmt_class.execute(params![
                            cls.class_name, cls.namespace, cls.base_classes.first(), file_id, cls.line as i64, cls.symbol_type, cls.end_line as i64
                        ]);
                        
                        let class_id_res: rusqlite::Result<i64> = stmt_class_id.query_row(
                            params![cls.class_name, file_id],
                            |row| row.get(0),
                        );

                        if let Ok(class_id) = class_id_res {
                            for parent in &cls.base_classes {
                                let _ = stmt_inheritance.execute(params![class_id, parent]);
                            }

                            for mem in &cls.members {
                                if mem.mem_type == "enum_item" {
                                    let _ = stmt_enum.execute(params![class_id, mem.name]);
                                } else {
                                    let is_static = if mem.flags.contains("static") { 1 } else { 0 };
                                    let _ = stmt_member.execute(params![
                                        class_id, mem.name, mem.mem_type, mem.flags, mem.access, mem.detail, mem.return_type, is_static, mem.line as i64
                                    ]);
                                }
                            }
                        }
                    }
                }
            }
        }
        tx.commit()?;
        current_idx = end_idx;
    }

    // Finalize: Integrate WAL faster
    reporter.report("finalizing", 50, 100, "Finalizing database (Integrating WAL)...");
    let _ = conn.pragma_update(None, "synchronous", "NORMAL"); 
    let _ = conn.execute("PRAGMA wal_checkpoint(RESTART)", []);
    
    reporter.report("finalizing", 90, 100, "Finalizing database (Optimizing)...");
    let _ = conn.execute("PRAGMA optimize", []);
    
    reporter.report("finalizing", 100, 100, "Database finalized.");
    
    Ok(())
}

pub fn get_module_id_for_path(conn: &Connection, file_path: &str) -> anyhow::Result<Option<i64>> {
    let mut stmt = conn.prepare(
        "SELECT id, root_path FROM modules ORDER BY length(root_path) DESC"
    )?;
    
    let mut rows = stmt.query([])?;
    let mut best_id = None;
    
    // 最も長く一致する root_path を持つモジュールを選択
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let root_path: String = row.get(1)?;
        if file_path.starts_with(&root_path) {
            best_id = Some(id);
            break;
        }
    }
    
    Ok(best_id)
}