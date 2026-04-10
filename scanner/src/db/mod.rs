pub mod path;

use std::path::Path;
use std::sync::Arc;
use std::collections::HashMap;
use rusqlite::{params, Connection};
use crate::types::{ParseResult, ProgressReporter};

pub const DB_VERSION: i32 = 14;

pub fn ensure_correct_version(db_path: &str) -> anyhow::Result<bool> {
    let mut version_match = false;
    let mut re_initialized = false;
    {
        if let Ok(conn) = rusqlite::Connection::open(db_path) {
            if let Ok(version_str) = conn.query_row(
                "SELECT value FROM project_meta WHERE key = 'db_version'",
                [],
                |row| row.get::<_, String>(0),
            ) {
                if let Ok(version) = version_str.parse::<i32>() {
                    if version == DB_VERSION { version_match = true; }
                }
            }
        }
    }

    if !version_match && Path::new(db_path).exists() {
        tracing::info!("DB version mismatch or missing (Current: {}). Re-initializing: {}", DB_VERSION, db_path);
        let _ = std::fs::remove_file(db_path);
        let conn = rusqlite::Connection::open(db_path)?;
        init_db(&conn)?;
        re_initialized = true;
    } else if !Path::new(db_path).exists() {
        let conn = rusqlite::Connection::open(db_path)?;
        init_db(&conn)?;
        re_initialized = true;
    }
    Ok(re_initialized)
}

pub fn init_db(conn: &Connection) -> rusqlite::Result<()> {
    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
    
    // 1. Tables (Create all tables first)
    conn.execute("CREATE TABLE IF NOT EXISTS strings (id INTEGER PRIMARY KEY AUTOINCREMENT, text TEXT NOT NULL UNIQUE COLLATE NOCASE)", [])?;
    
    conn.execute(
        "CREATE TABLE IF NOT EXISTS directories (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            parent_id INTEGER,
            name_id INTEGER NOT NULL,
            UNIQUE(parent_id, name_id),
            FOREIGN KEY(parent_id) REFERENCES directories(id) ON DELETE CASCADE,
            FOREIGN KEY(name_id) REFERENCES strings(id)
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS modules (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name_id INTEGER NOT NULL,
            type TEXT,
            scope TEXT,
            root_directory_id INTEGER NOT NULL,
            build_cs_path TEXT,
            owner_name TEXT,
            component_name TEXT,
            deep_dependencies TEXT,
            UNIQUE(name_id, root_directory_id),
            FOREIGN KEY(name_id) REFERENCES strings(id),
            FOREIGN KEY(root_directory_id) REFERENCES directories(id)
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            directory_id INTEGER NOT NULL,
            filename_id INTEGER NOT NULL,
            extension TEXT,
            mtime INTEGER,
            module_id INTEGER,
            is_header INTEGER DEFAULT 0,
            file_hash TEXT,
            UNIQUE(directory_id, filename_id),
            FOREIGN KEY(directory_id) REFERENCES directories(id) ON DELETE CASCADE,
            FOREIGN KEY(filename_id) REFERENCES strings(id),
            FOREIGN KEY(module_id) REFERENCES modules(id)
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS classes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name_id INTEGER NOT NULL,
            namespace_id INTEGER,
            base_class_id INTEGER,
            file_id INTEGER,
            line_number INTEGER,
            end_line_number INTEGER,
            symbol_type TEXT DEFAULT \"class\",
            FOREIGN KEY(name_id) REFERENCES strings(id),
            FOREIGN KEY(namespace_id) REFERENCES strings(id),
            FOREIGN KEY(base_class_id) REFERENCES strings(id),
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
        )",
        [],
    )?;
    
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

    conn.execute(
        "CREATE TABLE IF NOT EXISTS enum_values (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            enum_id INTEGER NOT NULL,
            name_id INTEGER NOT NULL,
            line_number INTEGER,
            file_id INTEGER,
            FOREIGN KEY(enum_id) REFERENCES classes(id) ON DELETE CASCADE,
            FOREIGN KEY(name_id) REFERENCES strings(id),
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS inheritance (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            child_id INTEGER NOT NULL,
            parent_name_id INTEGER NOT NULL,
            parent_class_id INTEGER,
            FOREIGN KEY(child_id) REFERENCES classes(id) ON DELETE CASCADE,
            FOREIGN KEY(parent_name_id) REFERENCES strings(id),
            FOREIGN KEY(parent_class_id) REFERENCES classes(id) ON DELETE SET NULL
        )",
        [],
    )?;

    let _ = conn.execute("CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(name, type, class_name UNINDEXED, rowid_ref UNINDEXED)", []);
    conn.execute("CREATE TABLE IF NOT EXISTS project_meta (key TEXT PRIMARY KEY, value TEXT)", [])?;
    
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

    conn.execute(
        "CREATE TABLE IF NOT EXISTS file_includes (
            file_id INTEGER NOT NULL,
            include_path_id INTEGER NOT NULL,
            base_filename_id INTEGER NOT NULL,
            resolved_file_id INTEGER,
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE,
            FOREIGN KEY(include_path_id) REFERENCES strings(id),
            FOREIGN KEY(base_filename_id) REFERENCES strings(id),
            FOREIGN KEY(resolved_file_id) REFERENCES files(id) ON DELETE SET NULL
        )",
        [],
    )?;

    // 2. Indices (Now create indices after all tables exist)
    create_indices(conn)?;

    conn.execute("INSERT OR REPLACE INTO project_meta (key, value) VALUES (\"db_version\", ?)", [DB_VERSION.to_string()])?;
    Ok(())
}

fn create_indices(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("CREATE INDEX IF NOT EXISTS idx_strings_text ON strings(text)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_directories_parent ON directories(parent_id)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_files_filename_id ON files(filename_id)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_files_dir_id ON files(directory_id)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_classes_covering ON classes(name_id, file_id, line_number, symbol_type)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_classes_file_id ON classes(file_id)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_members_name_id ON members(name_id)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_members_file_id ON members(file_id)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_file_includes_file_id ON file_includes(file_id)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_file_includes_resolved_id ON file_includes(resolved_file_id)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_file_includes_base_name ON file_includes(base_filename_id)", [])?;
    Ok(())
}

fn drop_indices(conn: &Connection) -> rusqlite::Result<()> {
    let indices = [
        "idx_strings_text", "idx_directories_parent",
        "idx_files_filename_id", "idx_files_dir_id", 
        "idx_classes_covering", "idx_classes_file_id",
        "idx_members_name_id", "idx_members_file_id",
        "idx_file_includes_file_id", "idx_file_includes_resolved_id", "idx_file_includes_base_name"
    ];
    for idx in indices {
        let _ = conn.execute(&format!("DROP INDEX IF EXISTS {}", idx), []);
    }
    Ok(())
}

pub fn get_or_create_string(tx: &rusqlite::Transaction, cache: &mut HashMap<String, i64>, text: &str) -> rusqlite::Result<i64> {
    let text = text.trim();
    if let Some(&id) = cache.get(text) { return Ok(id); }
    let id: i64 = match tx.query_row("SELECT id FROM strings WHERE text = ?", [text], |row| row.get(0)) {
        Ok(id) => id,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            tx.execute("INSERT INTO strings (text) VALUES (?)", [text])?;
            tx.last_insert_rowid()
        },
        Err(e) => return Err(e),
    };
    cache.insert(text.to_string(), id);
    Ok(id)
}

pub fn save_to_db(conn: &mut Connection, results: &[ParseResult], reporter: Arc<dyn ProgressReporter>) -> anyhow::Result<()> {
    // 最初にテーブルが存在することを保証する
    init_db(conn)?;

    conn.busy_timeout(std::time::Duration::from_millis(60000))?;
    let _ = conn.pragma_update(None, "journal_mode", "WAL");
    let _ = conn.pragma_update(None, "synchronous", "OFF"); 
    let _ = conn.pragma_update(None, "cache_size", "-800000"); 
    let _ = conn.pragma_update(None, "temp_store", "MEMORY");
    conn.execute("PRAGMA foreign_keys = OFF", [])?; 

    // 1. インデックスを削除
    reporter.report("db_sync", 0, 100, "Dropping indices for faster insertion...");
    let _ = drop_indices(conn);

    let total = results.len();
    reporter.report("db_sync", 0, total, &format!("Saving results (0/{})", total));

    let mut string_cache: HashMap<String, i64> = HashMap::new();
    let mut dir_cache: HashMap<(Option<i64>, i64), i64> = HashMap::new();

    // 2. 巨大な単一トランザクションを開始
    let tx = conn.transaction()?;
    {
        let mut stmt_del_file = tx.prepare("DELETE FROM files WHERE directory_id = ? AND filename_id = ?")?;
        let mut stmt_file = tx.prepare("INSERT INTO files (directory_id, filename_id, extension, mtime, file_hash, module_id, is_header) VALUES (?, ?, ?, ?, ?, ?, ?)")?;
        let mut stmt_class = tx.prepare("INSERT INTO classes (name_id, namespace_id, base_class_id, file_id, line_number, symbol_type, end_line_number) VALUES (?, ?, ?, ?, ?, ?, ?)")?;
        let mut stmt_inheritance = tx.prepare("INSERT INTO inheritance (child_id, parent_name_id) VALUES (?, ?)")?;
        let mut stmt_enum = tx.prepare("INSERT INTO enum_values (enum_id, name_id, line_number, file_id) VALUES (?, ?, ?, ?)")?;
        let mut stmt_member = tx.prepare("INSERT INTO members (class_id, name_id, type_id, flags, access, detail, return_type_id, is_static, line_number, file_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")?;
        let mut stmt_fts = tx.prepare("INSERT INTO symbols_fts (name, type, class_name, rowid_ref) VALUES (?, ?, ?, ?)")?;
        let mut stmt_include = tx.prepare("INSERT INTO file_includes (file_id, include_path_id, base_filename_id) VALUES (?, ?, ?)")?;

        for (i, result) in results.iter().enumerate() {
            if i % 500 == 0 {
                reporter.report("db_sync", i, total, &format!("Saving results ({}/{})", i, total));
            }
            if result.status != "parsed" { continue; }
            let data = match &result.data { Some(d) => d, None => continue };

            let path_obj = Path::new(&result.path);
            let parent_dir = path_obj.parent().unwrap_or(Path::new(""));
            let filename = path_obj.file_name().and_then(|s| s.to_str()).unwrap_or("unknown");
            let extension = path_obj.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
            
            let dir_id = path::get_or_create_directory(&tx, &mut string_cache, &mut dir_cache, parent_dir)?;
            let filename_id = get_or_create_string(&tx, &mut string_cache, filename)?;
            
            let _ = stmt_del_file.execute(params![dir_id, filename_id]);

            if stmt_file.execute(params![
                dir_id, filename_id, extension, result.mtime as i64, data.new_hash, result.module_id,
                if extension == "h" || extension == "hpp" { 1 } else { 0 }
            ]).is_ok() {
                let file_id: i64 = tx.last_insert_rowid();
                for cls in &data.classes {
                    let cls_id = get_or_create_string(&tx, &mut string_cache, &cls.class_name)?;
                    let ns_id = match &cls.namespace { Some(ns) => Some(get_or_create_string(&tx, &mut string_cache, ns)?), None => None };
                    let base_id = match cls.base_classes.first() { Some(b) => Some(get_or_create_string(&tx, &mut string_cache, b)?), None => None };
                    let _ = stmt_class.execute(params![cls_id, ns_id, base_id, file_id, cls.line as i64, cls.symbol_type, cls.end_line as i64]);
                    let class_id: i64 = tx.last_insert_rowid();
                    let _ = stmt_fts.execute(params![cls.class_name, cls.symbol_type, cls.class_name, class_id]);
                    for parent in &cls.base_classes {
                        let p_name_id = get_or_create_string(&tx, &mut string_cache, parent)?;
                        let _ = stmt_inheritance.execute(params![class_id, p_name_id]);
                    }
                    for mem in &cls.members {
                        let mem_name_id = get_or_create_string(&tx, &mut string_cache, &mem.name)?;
                        if mem.mem_type == "enum_item" {
                            let _ = stmt_enum.execute(params![class_id, mem_name_id, mem.line as i64, file_id]);
                        } else {
                            let rt_id = match &mem.return_type { Some(rt) => Some(get_or_create_string(&tx, &mut string_cache, rt)?), None => None };
                            let type_id = get_or_create_string(&tx, &mut string_cache, &mem.mem_type)?;
                            let _ = stmt_member.execute(params![class_id, mem_name_id, type_id, mem.flags, mem.access, mem.detail, rt_id, if mem.flags.contains("static") {1} else {0}, mem.line as i64, file_id]);
                            let _ = stmt_fts.execute(params![mem.name, mem.mem_type, cls.class_name, tx.last_insert_rowid()]);
                        }
                    }
                }
                for inc in &data.includes {
                    let inc_path_id = get_or_create_string(&tx, &mut string_cache, inc)?;
                    let inc_fn = Path::new(inc).file_name().and_then(|s| s.to_str()).unwrap_or(inc);
                    let inc_fn_id = get_or_create_string(&tx, &mut string_cache, inc_fn)?;
                    let _ = stmt_include.execute(params![file_id, inc_path_id, inc_fn_id]);
                }
            }
        }
    }
    tx.commit()?; // トランザクションをコミット

    // 3. インデックスを再構築
    reporter.report("finalizing", 70, 100, "Re-creating indices (this may take a while)...");
    create_indices(conn)?;

    conn.execute("PRAGMA foreign_keys = ON", [])?; 

    reporter.report("finalizing", 80, 100, "Optimizing inheritance graph...");
    let _ = conn.execute("UPDATE inheritance SET parent_class_id = (SELECT c.id FROM classes c JOIN strings s ON c.name_id = s.id WHERE s.id = inheritance.parent_name_id LIMIT 1) WHERE parent_class_id IS NULL", []);
    reporter.report("finalizing", 85, 100, "Resolving file includes...");
    let _ = conn.execute("UPDATE file_includes SET resolved_file_id = (SELECT f.id FROM files f WHERE f.filename_id = file_includes.base_filename_id LIMIT 1) WHERE resolved_file_id IS NULL", []);
    reporter.report("finalizing", 95, 100, "Vacuuming and optimizing...");
    let _ = conn.execute("PRAGMA optimize", []);
    Ok(())
}

pub fn register_module(conn: &Connection, name: &str, root_path: &str, m_type: &str, scope: &str) -> anyhow::Result<i64> {
    let tx = conn.unchecked_transaction()?;
    let mut str_cache = HashMap::new();
    let mut dir_cache = HashMap::new();
    let name_id = get_or_create_string(&tx, &mut str_cache, name)?;
    let root_dir_id = path::get_or_create_directory(&tx, &mut str_cache, &mut dir_cache, Path::new(root_path))?;
    tx.execute("INSERT OR REPLACE INTO modules (name_id, root_directory_id, type, scope) VALUES (?, ?, ?, ?)", params![name_id, root_dir_id, m_type, scope])?;
    let id = tx.last_insert_rowid();
    tx.commit()?;
    Ok(id)
}

pub fn get_module_id_for_path(conn: &Connection, file_path: &str) -> anyhow::Result<Option<i64>> {
    let mut stmt = conn.prepare("SELECT m.id, m.root_directory_id FROM modules m")?;
    let mut rows = stmt.query([])?;
    let mut best_id = None;
    let mut best_len = 0;
    let file_path_norm = file_path.replace('\\', "/").to_lowercase();
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let root_dir_id: i64 = row.get(1)?;
        if let Ok(root_path) = path::get_full_path(conn, root_dir_id, 0) {
            let root_path_norm = root_path.replace('\\', "/").to_lowercase();
            if file_path_norm.starts_with(&root_path_norm) && root_path_norm.len() > best_len {
                best_id = Some(id);
                best_len = root_path_norm.len();
            }
        }
    }
    Ok(best_id)
}

pub fn get_components(conn: &Connection) -> anyhow::Result<serde_json::Value> {
    let mut stmt = conn.prepare("SELECT name, display_name, type, owner_name, root_path, uplugin_path, uproject_path, engine_association FROM components")?;
    let rows = stmt.query_map([], |row| {
        Ok(serde_json::json!({
            "name": row.get::<_, String>(0)?,
            "display_name": row.get::<_, String>(1)?,
            "type": row.get::<_, String>(2)?,
            "owner_name": row.get::<_, String>(3)?,
            "root_path": row.get::<_, String>(4)?,
            "uplugin_path": row.get::<_, Option<String>>(5)?,
            "uproject_path": row.get::<_, Option<String>>(6)?,
            "engine_association": row.get::<_, Option<String>>(7)?,
        }))
    })?;
    let results: Vec<_> = rows.filter_map(|r| r.ok()).collect();
    Ok(serde_json::json!(results))
}

pub fn init_cache_db(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("CREATE TABLE IF NOT EXISTS persistent_cache (key TEXT PRIMARY KEY, value BLOB NOT NULL, hit_count INTEGER DEFAULT 1, last_used INTEGER NOT NULL)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_cache_last_used ON persistent_cache(last_used)", [])?;
    Ok(())
}
