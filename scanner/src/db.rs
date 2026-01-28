use std::path::Path;
use rusqlite::{params, Connection};
use crate::types::ParseResult;

pub fn init_db(conn: &Connection) -> rusqlite::Result<()> {
    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
    
    // 1. Modules
    conn.execute(
        "CREATE TABLE IF NOT EXISTS modules (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            type TEXT,
            scope TEXT,
            root_path TEXT NOT NULL,
            build_cs_path TEXT,
            owner_name TEXT,
            component_name TEXT,
            deep_dependencies TEXT,
            UNIQUE(name, root_path)
        )",
        [],
    )?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_modules_name ON modules(name)", [])?;

    // 2. Files
    conn.execute(
        "CREATE TABLE IF NOT EXISTS files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT NOT NULL UNIQUE,
            filename TEXT NOT NULL,
            extension TEXT,
            mtime INTEGER,
            module_id INTEGER,
            is_header INTEGER DEFAULT 0,
            file_hash TEXT,
            FOREIGN KEY(module_id) REFERENCES modules(id) ON DELETE CASCADE
        )",
        [],
    )?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_files_filename ON files(filename)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_files_module_id ON files(module_id)", [])?;

    // 3. Classes
    conn.execute(
        "CREATE TABLE IF NOT EXISTS classes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            namespace TEXT,
            base_class TEXT,
            file_id INTEGER,
            line_number INTEGER,
            symbol_type TEXT DEFAULT 'class',
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
        )",
        [],
    )?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_classes_name ON classes(name)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_classes_base_class ON classes(base_class)", [])?;
    conn.execute("CREATE UNIQUE INDEX IF NOT EXISTS idx_classes_unique_name_file ON classes(name, symbol_type, namespace, file_id)", [])?;

    // 4. Members
    conn.execute(
        "CREATE TABLE IF NOT EXISTS members (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            class_id INTEGER NOT NULL,
            name TEXT NOT NULL,
            type TEXT NOT NULL,
            flags TEXT,
            access TEXT,
            detail TEXT,
            return_type TEXT,
            is_static INTEGER,
            FOREIGN KEY(class_id) REFERENCES classes(id) ON DELETE CASCADE
        )",
        [],
    )?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_members_name ON members(name)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_members_class_id ON members(class_id)", [])?;

    // 5. Enum Values
    conn.execute(
        "CREATE TABLE IF NOT EXISTS enum_values (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            enum_id INTEGER NOT NULL,
            name TEXT NOT NULL,
            FOREIGN KEY(enum_id) REFERENCES classes(id) ON DELETE CASCADE
        )",
        [],
    )?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_enum_values_id ON enum_values(enum_id)", [])?;

    // 6. Inheritance
    conn.execute(
        "CREATE TABLE IF NOT EXISTS inheritance (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            child_id INTEGER NOT NULL,
            parent_name TEXT NOT NULL,
            FOREIGN KEY(child_id) REFERENCES classes(id) ON DELETE CASCADE
        )",
        [],
    )?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_inheritance_child ON inheritance(child_id)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_inheritance_parent ON inheritance(parent_name)", [])?;

    // 7. Project Meta
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

    Ok(())
}

pub fn save_to_db(db_path: &str, results: &[ParseResult]) -> anyhow::Result<()> {
    let mut conn = Connection::open(db_path)?;
    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.execute("PRAGMA foreign_keys = ON", [])?;

    let total = results.len();
    crate::types::report_progress("db_sync", 0, 100, &format!("Saving results to DB (0/{})", total));

    let tx = conn.transaction()?;
    for (i, result) in results.iter().enumerate() {
        if i % 100 == 0 {
            crate::types::report_progress("db_sync", i * 100 / total, 100, &format!("Saving results to DB ({}/{})", i, total));
        }
        if result.status != "parsed" { continue; }
        let data = match &result.data {
            Some(d) => d,
            None => continue,
        };

        // 1. ファイル情報の登録/更新 (CASCADE)
        let _ = tx.execute("DELETE FROM files WHERE path = ?", params![result.path]);
        
        let path_obj = Path::new(&result.path);
        let filename = path_obj.file_name().and_then(|s| s.to_str()).unwrap_or("unknown");
        let extension = path_obj.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
        
        let safe_module_id = if let Some(id) = result.module_id {
            if id <= 0 { None } else { Some(id) }
        } else {
            None
        };

        let file_res = tx.execute(
            "INSERT INTO files (path, filename, extension, mtime, file_hash, module_id, is_header) VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                result.path,
                filename,
                extension,
                result.mtime as i64,
                data.new_hash,
                safe_module_id,
                if extension == "h" || extension == "hpp" { 1 } else { 0 }
            ],
        );

        if file_res.is_ok() {
            let file_id: i64 = tx.last_insert_rowid();

            // 2. クラス情報の登録
            for cls in &data.classes {
                let _ = tx.execute(
                    "INSERT OR IGNORE INTO classes (name, namespace, base_class, file_id, line_number, symbol_type) VALUES (?, ?, ?, ?, ?, ?)",
                    params![
                        cls.class_name,
                        cls.namespace,
                        cls.base_classes.first(),
                        file_id,
                        cls.line as i64,
                        cls.symbol_type
                    ],
                );
                
                let class_id_res: rusqlite::Result<i64> = tx.query_row(
                    "SELECT id FROM classes WHERE name = ? AND file_id = ? LIMIT 1",
                    params![cls.class_name, file_id],
                    |row| row.get(0),
                );

                if let Ok(class_id) = class_id_res {
                    // 3. 継承関係
                    for parent in &cls.base_classes {
                        let _ = tx.execute(
                            "INSERT INTO inheritance (child_id, parent_name) VALUES (?, ?)",
                            params![class_id, parent],
                        );
                    }

                    // 4. メンバー
                    for mem in &cls.members {
                        if mem.mem_type == "enum_item" {
                            let _ = tx.execute(
                                "INSERT INTO enum_values (enum_id, name) VALUES (?, ?)",
                                params![class_id, mem.name],
                            );
                        } else {
                            let is_static = if mem.flags.contains("static") { 1 } else { 0 };
                            let _ = tx.execute(
                                "INSERT INTO members (class_id, name, type, flags, access, detail, return_type, is_static) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                                params![
                                    class_id,
                                    mem.name,
                                    mem.mem_type,
                                    mem.flags,
                                    "public",
                                    mem.detail,
                                    mem.return_type,
                                    is_static
                                ],
                            );
                        }
                    }
                }
            }
        }
    }

    tx.commit()?;
    Ok(())
}
