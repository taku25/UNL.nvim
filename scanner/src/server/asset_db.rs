//! asset_db.rs — Persistent SQLite cache for the asset graph.
//!
//! Stores parsed uasset/umap data with mtimes so subsequent scans can skip
//! unchanged files.  The DB lives alongside the main symbol DB:
//!   <main_db>_asset_cache.db
//!
//! Schema version is checked on every open; a mismatch drops and recreates all
//! tables (safe — data is always re-parseable from source files).

use std::collections::HashMap;
use std::sync::Arc;
use rusqlite::{Connection, params};
use tracing::info;
use crate::server::state::AssetGraph;

/// Bump when schema or stored data format changes.
pub const ASSET_CACHE_VERSION: i32 = 1;

// ─────────────────────────────────────────────────────────────────────────────
// Data types
// ─────────────────────────────────────────────────────────────────────────────

pub struct AssetCacheRow {
    pub file_path:    String,           // native absolute path (primary key)
    pub asset_path:   String,           // /Game/... logical path
    pub mtime:        i64,              // Unix seconds
    pub parent_class: Option<String>,
    pub imports:      Vec<String>,
    pub functions:    Vec<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// DB helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Derive the asset cache DB path from the main symbol DB path.
/// Works with both unix-normalized and native path strings.
pub fn asset_db_path(main_db_path: &str) -> String {
    if let Some(stripped) = main_db_path.strip_suffix(".db") {
        format!("{}_asset_cache.db", stripped)
    } else {
        format!("{}_asset_cache.db", main_db_path)
    }
}

/// Open (or create) the asset cache DB and ensure the schema is current.
pub fn open_asset_db(path: &str) -> anyhow::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    let _ = conn.pragma_update(None, "journal_mode", "WAL");
    let _ = conn.pragma_update(None, "synchronous", "NORMAL");
    let _ = conn.pragma_update(None, "temp_store", "MEMORY");
    ensure_schema(&conn)?;
    Ok(conn)
}

fn ensure_schema(conn: &Connection) -> anyhow::Result<()> {
    // Read stored version (may not exist yet).
    let stored: Option<i32> = conn
        .query_row(
            "SELECT value FROM asset_meta WHERE key = 'version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|s| s.parse().ok());

    if stored == Some(ASSET_CACHE_VERSION) {
        return Ok(());
    }

    // Version mismatch or first run — recreate tables.
    conn.execute_batch(
        "DROP TABLE IF EXISTS assets;
         DROP TABLE IF EXISTS asset_meta;
         CREATE TABLE asset_meta (key TEXT PRIMARY KEY, value TEXT);
         CREATE TABLE assets (
             file_path    TEXT PRIMARY KEY,
             asset_path   TEXT NOT NULL,
             mtime        INTEGER NOT NULL,
             parent_class TEXT,
             imports      TEXT NOT NULL DEFAULT '[]',
             functions    TEXT NOT NULL DEFAULT '[]'
         );
         CREATE INDEX IF NOT EXISTS idx_assets_asset_path ON assets(asset_path);",
    )?;
    conn.execute(
        "INSERT OR REPLACE INTO asset_meta (key, value) VALUES ('version', ?1)",
        params![ASSET_CACHE_VERSION.to_string()],
    )?;
    info!("Asset cache DB initialized (version {}): {:?}", ASSET_CACHE_VERSION,
          conn.path().unwrap_or("?"));
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// CRUD helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Load all rows.  Returns a map keyed by native file_path for O(1) lookup.
pub fn load_all_by_path(conn: &Connection) -> anyhow::Result<HashMap<String, AssetCacheRow>> {
    let mut stmt = conn.prepare(
        "SELECT file_path, asset_path, mtime, parent_class, imports, functions FROM assets",
    )?;
    let mut map = HashMap::new();
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
        ))
    })?;
    for r in rows.filter_map(|r| r.ok()) {
        let (file_path, asset_path, mtime, parent_class, imports_json, functions_json) = r;
        let imports   = serde_json::from_str::<Vec<String>>(&imports_json).unwrap_or_default();
        let functions = serde_json::from_str::<Vec<String>>(&functions_json).unwrap_or_default();
        map.insert(file_path.clone(), AssetCacheRow { file_path, asset_path, mtime, parent_class, imports, functions });
    }
    Ok(map)
}

/// Upsert a batch of rows in a single transaction.
pub fn upsert_batch(conn: &mut Connection, rows: &[AssetCacheRow]) -> anyhow::Result<()> {
    if rows.is_empty() { return Ok(()); }
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR REPLACE INTO assets
             (file_path, asset_path, mtime, parent_class, imports, functions)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for row in rows {
            let imports_j   = serde_json::to_string(&row.imports).unwrap_or_default();
            let functions_j = serde_json::to_string(&row.functions).unwrap_or_default();
            stmt.execute(params![
                row.file_path, row.asset_path, row.mtime,
                row.parent_class, imports_j, functions_j,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Upsert a single row (used for incremental watcher updates).
pub fn upsert_one(conn: &Connection, row: &AssetCacheRow) -> anyhow::Result<()> {
    let imports_j   = serde_json::to_string(&row.imports).unwrap_or_default();
    let functions_j = serde_json::to_string(&row.functions).unwrap_or_default();
    conn.execute(
        "INSERT OR REPLACE INTO assets
         (file_path, asset_path, mtime, parent_class, imports, functions)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            row.file_path, row.asset_path, row.mtime,
            row.parent_class, imports_j, functions_j,
        ],
    )?;
    Ok(())
}

/// Delete stale entries (files that no longer exist on disk).
pub fn delete_batch(conn: &mut Connection, file_paths: &[String]) -> anyhow::Result<()> {
    if file_paths.is_empty() { return Ok(()); }
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare("DELETE FROM assets WHERE file_path = ?1")?;
        for fp in file_paths {
            stmt.execute(params![fp])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Delete a single entry.
pub fn delete_one(conn: &Connection, file_path: &str) -> anyhow::Result<()> {
    conn.execute("DELETE FROM assets WHERE file_path = ?1", params![file_path])?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Graph builder
// ─────────────────────────────────────────────────────────────────────────────

/// Reconstruct an AssetGraph from a map of cache rows (no file I/O needed).
pub fn build_graph(rows: &HashMap<String, AssetCacheRow>) -> AssetGraph {
    let mut graph = AssetGraph::default();
    for row in rows.values() {
        let ap: Arc<str> = row.asset_path.to_lowercase().into();
        if let Some(ref p) = row.parent_class {
            graph.derived.entry(p.to_lowercase().into()).or_default().insert(ap.clone());
        }
        for imp in &row.imports {
            graph.references.entry(imp.to_lowercase().into()).or_default().insert(ap.clone());
        }
        for func in &row.functions {
            graph.functions.entry(func.to_lowercase().into()).or_default().insert(ap.clone());
        }
    }
    graph
}
