use std::path::Path;
use std::sync::Arc;
use std::collections::HashMap;
use rusqlite::{params, Connection};
use crate::types::{ParseResult, ProgressReporter};

pub const DB_VERSION: i32 = 6;

/// 指定されたDBファイルが最新バージョンであることを保証する。
/// バージョンが合わない場合はファイルを削除して初期化する。
pub fn ensure_correct_version(db_path: &str) -> anyhow::Result<()> {
    let mut version_match = false;
    {
        if let Ok(conn) = rusqlite::Connection::open(db_path) {
            if let Ok(version_str) = conn.query_row(
                "SELECT value FROM project_meta WHERE key = 'db_version'",
                [],
                |row| row.get::<_, String>(0),
            ) {
                if let Ok(version) = version_str.parse::<i32>() {
                    if version == DB_VERSION {
                        version_match = true;
                    }
                }
            }
        }
    }

    if !version_match && Path::new(db_path).exists() {
        tracing::info!("DB version mismatch or missing (Current: 6). Re-initializing: {}", db_path);
        let _ = std::fs::remove_file(db_path);
        let conn = rusqlite::Connection::open(db_path)?;
        init_db(&conn)?;
    } else if !Path::new(db_path).exists() {
        let conn = rusqlite::Connection::open(db_path)?;
        init_db(&conn)?;
    }

    Ok(())
}

pub fn init_db(conn: &Connection) -> rusqlite::Result<()> {
    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
    
    // 0. String Interning Table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS strings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            text TEXT NOT NULL UNIQUE
        )",
        [],
    )?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_strings_text ON strings(text)", [])?;

    // 1. Modules
    conn.execute(
        "CREATE TABLE IF NOT EXISTS modules (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name_id INTEGER NOT NULL,
            type TEXT,
            scope TEXT,
            root_path_id INTEGER NOT NULL,
            build_cs_path TEXT,
            owner_name TEXT,
            component_name TEXT,
            deep_dependencies TEXT,
            UNIQUE(name_id, root_path_id),
            FOREIGN KEY(name_id) REFERENCES strings(id),
            FOREIGN KEY(root_path_id) REFERENCES strings(id)
        )",
        [],
    )?;

    // 2. Files
    conn.execute(
        "CREATE TABLE IF NOT EXISTS files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path_id INTEGER NOT NULL UNIQUE,
            filename_id INTEGER NOT NULL,
            extension TEXT,
            mtime INTEGER,
            module_id INTEGER,
            is_header INTEGER DEFAULT 0,
            file_hash TEXT,
            FOREIGN KEY(path_id) REFERENCES strings(id),
            FOREIGN KEY(filename_id) REFERENCES strings(id),
            FOREIGN KEY(module_id) REFERENCES modules(id) ON DELETE CASCADE
        )",
        [],
    )?;

    // 3. Classes
    conn.execute(
        "CREATE TABLE IF NOT EXISTS classes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name_id INTEGER NOT NULL,
            namespace_id INTEGER,
            base_class_id INTEGER,
            file_id INTEGER,
            line_number INTEGER,
            end_line_number INTEGER,
            symbol_type TEXT DEFAULT 'class',
            FOREIGN KEY(name_id) REFERENCES strings(id),
            FOREIGN KEY(namespace_id) REFERENCES strings(id),
            FOREIGN KEY(base_class_id) REFERENCES strings(id),
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
        )",
        [],
    )?;
    // Covering Index: 名前検索時にファイルID、行番号、シンボルタイプを即座に取得
    conn.execute("CREATE INDEX IF NOT EXISTS idx_classes_covering ON classes(name_id, file_id, line_number, symbol_type)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_classes_file_id ON classes(file_id)", [])?;
    
    // 4. Members
    conn.execute(
        "CREATE TABLE IF NOT EXISTS members (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            class_id INTEGER NOT NULL,
            name_id INTEGER NOT NULL,
            type_id INTEGER NOT NULL,
            flags TEXT,
            access TEXT,
            detail TEXT,
            return_type_id INTEGER,
            is_static INTEGER,
            line_number INTEGER,
            file_id INTEGER,
            FOREIGN KEY(class_id) REFERENCES classes(id) ON DELETE CASCADE,
            FOREIGN KEY(name_id) REFERENCES strings(id),
            FOREIGN KEY(type_id) REFERENCES strings(id),
            FOREIGN KEY(return_type_id) REFERENCES strings(id),
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
        )",
        [],
    )?;
    // Covering Index for member listing
    conn.execute("CREATE INDEX IF NOT EXISTS idx_members_covering ON members(class_id, name_id, type_id, line_number, access, flags)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_members_name_id ON members(name_id)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_members_file_id ON members(file_id)", [])?;

    // 5. Enum Values
    conn.execute(
        "CREATE TABLE IF NOT EXISTS enum_values (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            enum_id INTEGER NOT NULL,
            name_id INTEGER NOT NULL,
            FOREIGN KEY(enum_id) REFERENCES classes(id) ON DELETE CASCADE,
            FOREIGN KEY(name_id) REFERENCES strings(id)
        )",
        [],
    )?;
    conn.execute("CREATE UNIQUE INDEX IF NOT EXISTS idx_enum_values_unique ON enum_values(enum_id, name_id)", [])?;

    // 6. Inheritance
    conn.execute(
        "CREATE TABLE IF NOT EXISTS inheritance (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            child_id INTEGER NOT NULL,
            parent_name_id INTEGER NOT NULL,
            parent_class_id INTEGER, -- ★高速化：親クラスの直接ID
            FOREIGN KEY(child_id) REFERENCES classes(id) ON DELETE CASCADE,
            FOREIGN KEY(parent_name_id) REFERENCES strings(id),
            FOREIGN KEY(parent_class_id) REFERENCES classes(id) ON DELETE SET NULL
        )",
        [],
    )?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_inheritance_child_id ON inheritance(child_id)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_inheritance_parent_class_id ON inheritance(parent_class_id)", [])?;

    // 7. FTS5 Virtual Table for Symbols (Lightning fast searching)
    // NOTE: FTS5 requires SQLite to be compiled with it, which is standard in most modern distros.
    let _ = conn.execute("CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(name, type, class_name UNINDEXED, rowid_ref UNINDEXED)", []);

    // 8. Project Meta
    conn.execute(
        "CREATE TABLE IF NOT EXISTS project_meta (
            key TEXT PRIMARY KEY,
            value TEXT
        )",
        [],
    )?;

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

    // 9. Symbol Calls (For Find Usages in C++)
    conn.execute(
        "CREATE TABLE IF NOT EXISTS symbol_calls (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_id INTEGER NOT NULL,
            line INTEGER NOT NULL,
            name_id INTEGER NOT NULL,
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE,
            FOREIGN KEY(name_id) REFERENCES strings(id)
        )",
        [],
    )?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_symbol_calls_name_id ON symbol_calls(name_id)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_symbol_calls_file_id ON symbol_calls(file_id)", [])?;

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
    let _ = conn.pragma_update(None, "synchronous", "OFF"); 
    let _ = conn.pragma_update(None, "cache_size", "-800000"); 
    let _ = conn.pragma_update(None, "temp_store", "MEMORY");
    
    conn.execute("PRAGMA foreign_keys = ON", [])?; 

    let total = results.len();
    reporter.report("db_sync", 0, total, &format!("Saving to DB (0/{})", total));

    // 文字列キャッシュ
    let mut string_cache: HashMap<String, i64> = HashMap::new();

    let get_or_create_string = |tx: &rusqlite::Transaction, cache: &mut HashMap<String, i64>, text: &str| -> rusqlite::Result<i64> {
        let text_trimmed = text.trim();
        if let Some(&id) = cache.get(text_trimmed) {
            return Ok(id);
        }
        
        let id: i64 = match tx.query_row("SELECT id FROM strings WHERE text = ?", [text_trimmed], |row| row.get(0)) {
            Ok(id) => id,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                tx.execute("INSERT INTO strings (text) VALUES (?)", [text_trimmed])?;
                tx.last_insert_rowid()
            },
            Err(e) => return Err(e),
        };
        
        cache.insert(text_trimmed.to_string(), id);
        Ok(id)
    };

    const BATCH_SIZE: usize = 2000;
    let mut current_idx = 0;

    while current_idx < total {
        let end_idx = std::cmp::min(current_idx + BATCH_SIZE, total);
        let batch = &results[current_idx..end_idx];

        let tx = conn.transaction()?;
        {
            // 更新対象のファイルを一旦削除して、CASCADEによって関連データをクリーンにする
            let mut stmt_del_file = tx.prepare("DELETE FROM files WHERE path_id = ?")?;
            let mut stmt_file = tx.prepare("INSERT INTO files (path_id, filename_id, extension, mtime, file_hash, module_id, is_header) VALUES (?, ?, ?, ?, ?, ?, ?)")?;
            let mut stmt_class = tx.prepare("INSERT INTO classes (name_id, namespace_id, base_class_id, file_id, line_number, symbol_type, end_line_number) VALUES (?, ?, ?, ?, ?, ?, ?)")?;
            let mut stmt_inheritance = tx.prepare("INSERT INTO inheritance (child_id, parent_name_id) VALUES (?, ?)")?;
            let mut stmt_enum = tx.prepare("INSERT INTO enum_values (enum_id, name_id) VALUES (?, ?)")?;
            let mut stmt_member = tx.prepare("INSERT INTO members (class_id, name_id, type_id, flags, access, detail, return_type_id, is_static, line_number, file_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")?;
            let mut stmt_fts = tx.prepare("INSERT INTO symbols_fts (name, type, class_name, rowid_ref) VALUES (?, ?, ?, ?)")?;

            for (_i, result) in batch.iter().enumerate() {
                let global_i = current_idx + _i;
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

                let path_id = get_or_create_string(&tx, &mut string_cache, &result.path)?;
                let filename_id = get_or_create_string(&tx, &mut string_cache, filename)?;
                
                let _ = stmt_del_file.execute([path_id]);

                let file_res = stmt_file.execute(params![
                    path_id, filename_id, extension, result.mtime as i64, data.new_hash, safe_module_id,
                    if extension == "h" || extension == "hpp" { 1 } else { 0 }
                ]);

                if file_res.is_ok() {
                    let file_id: i64 = tx.last_insert_rowid();

                    for cls in &data.classes {
                        let cls_name_id = get_or_create_string(&tx, &mut string_cache, &cls.class_name)?;
                        let ns_id = match &cls.namespace {
                            Some(ns) => Some(get_or_create_string(&tx, &mut string_cache, ns)?),
                            None => None,
                        };
                        let base_id = match cls.base_classes.first() {
                            Some(base) => Some(get_or_create_string(&tx, &mut string_cache, base)?),
                            None => None,
                        };

                        let _ = stmt_class.execute(params![
                            cls_name_id, ns_id, base_id, file_id, cls.line as i64, cls.symbol_type, cls.end_line as i64
                        ]);
                        
                        let class_id: i64 = tx.last_insert_rowid();
                        
                        // FTS Index: Class
                        let _ = stmt_fts.execute(params![cls.class_name, cls.symbol_type, cls.class_name, class_id]);

                        for parent in &cls.base_classes {
                            let p_name_id = get_or_create_string(&tx, &mut string_cache, parent)?;
                            let _ = stmt_inheritance.execute(params![class_id, p_name_id]);
                        }

                        for mem in &cls.members {
                            let mem_name_id = get_or_create_string(&tx, &mut string_cache, &mem.name)?;
                            if mem.mem_type == "enum_item" {
                                let _ = stmt_enum.execute(params![class_id, mem_name_id]);
                            } else {
                                let is_static = if mem.flags.contains("static") { 1 } else { 0 };
                                let type_id = get_or_create_string(&tx, &mut string_cache, &mem.mem_type)?;
                                let rt_id = match &mem.return_type {
                                    Some(rt) => Some(get_or_create_string(&tx, &mut string_cache, rt)?),
                                    None => None,
                                };

                                let _ = stmt_member.execute(params![
                                    class_id, mem_name_id, type_id, mem.flags, mem.access, mem.detail, rt_id, is_static, mem.line as i64, file_id
                                ]);
                                
                                // FTS Index: Member
                                let _ = stmt_fts.execute(params![mem.name, mem.mem_type, cls.class_name, tx.last_insert_rowid()]);
                            }
                        }
                    }
                }
            }
        }
        tx.commit()?;
        current_idx = end_idx;
    }

    // 後処理：継承関係のID解決を一括で行う
    reporter.report("finalizing", 80, 100, "Optimizing inheritance graph...");
    conn.execute(
        "UPDATE inheritance SET parent_class_id = (
            SELECT c.id FROM classes c 
            JOIN strings s ON c.name_id = s.id 
            WHERE s.id = inheritance.parent_name_id 
            LIMIT 1
        ) WHERE parent_class_id IS NULL",
        [],
    )?;

    // Finalize
    reporter.report("finalizing", 90, 100, "Vacuuming and optimizing...");
    let _ = conn.execute("ANALYZE", []);
    let _ = conn.execute("PRAGMA optimize", []);
    reporter.report("finalizing", 100, 100, "Database finalized.");
    
    Ok(())
}

pub fn get_module_id_for_path(_conn: &Connection, file_path: &str) -> anyhow::Result<Option<i64>> {
    let mut stmt = _conn.prepare(
        "SELECT m.id, s.text
         FROM modules m
         JOIN strings s ON m.root_path_id = s.id
         ORDER BY length(s.text) DESC"
    )?;

    let mut rows = stmt.query([])?;
    let mut best_id = None;

    let file_path_lower = if cfg!(target_os = "windows") {
        file_path.to_lowercase()
    } else {
        String::new() // Not used on non-windows
    };

    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let root_path: String = row.get(1)?;

        let matched = if cfg!(target_os = "windows") {
            file_path_lower.starts_with(&root_path.to_lowercase())
        } else {
            file_path.starts_with(&root_path)
        };

        if matched {
            best_id = Some(id);
            break;
        }
    }

    Ok(best_id)
}
pub fn register_module(conn: &Connection, name: &str, root_path: &str, m_type: &str, scope: &str) -> anyhow::Result<i64> {
    let tx = conn.unchecked_transaction()?;
    
    let mut string_cache = HashMap::new();
    let get_id = |tx: &rusqlite::Transaction, cache: &mut HashMap<String, i64>, t: &str| -> rusqlite::Result<i64> {
        let t = t.trim();
        if let Some(&id) = cache.get(t) { return Ok(id); }
        let id: i64 = match tx.query_row("SELECT id FROM strings WHERE text = ?", [t], |r| r.get(0)) {
            Ok(id) => id,
            Err(_) => {
                tx.execute("INSERT INTO strings (text) VALUES (?)", [t])?;
                tx.last_insert_rowid()
            }
        };
        cache.insert(t.to_string(), id);
        Ok(id)
    };

    let name_id = get_id(&tx, &mut string_cache, name)?;
    let root_id = get_id(&tx, &mut string_cache, root_path)?;

    tx.execute(
        "INSERT OR REPLACE INTO modules (name_id, root_path_id, type, scope) VALUES (?, ?, ?, ?)",
        params![name_id, root_id, m_type, scope],
    )?;
    let id = tx.last_insert_rowid();
    tx.commit()?;
    Ok(id)
}