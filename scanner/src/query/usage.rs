use rusqlite::Connection;
use rusqlite::types::ToSql;
use rusqlite::OptionalExtension;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::BufRead;
use crate::db::path::{PATH_CTE, to_db_path_format};

const MAX_RESULTS: usize = 300;
const MAX_FILES: usize = 2000;

fn find_definition_file_ids(conn: &Connection, symbol_name: &str) -> anyhow::Result<Vec<i64>> {
    let mut ids: Vec<i64> = Vec::new();
    let mut seen: HashSet<i64> = HashSet::new();

    // classes テーブルから
    {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT c.file_id FROM classes c
             JOIN strings s ON c.name_id = s.id
             WHERE s.text = ? AND c.file_id IS NOT NULL",
        )?;
        let mut rows = stmt.query(rusqlite::params![symbol_name])?;
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            if seen.insert(id) {
                ids.push(id);
            }
        }
    }

    // members テーブルから
    {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT m.file_id FROM members m
             JOIN strings s ON m.name_id = s.id
             WHERE s.text = ? AND m.file_id IS NOT NULL",
        )?;
        let mut rows = stmt.query(rusqlite::params![symbol_name])?;
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            if seen.insert(id) {
                ids.push(id);
            }
        }
    }

    Ok(ids)
}

fn find_including_file_ids(conn: &Connection, def_ids: &[i64]) -> anyhow::Result<HashSet<i64>> {
    let mut result: HashSet<i64> = HashSet::new();
    if def_ids.is_empty() {
        return Ok(result);
    }

    for chunk in def_ids.chunks(50) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT DISTINCT fi.file_id FROM file_includes fi WHERE fi.resolved_file_id IN ({})",
            placeholders
        );
        let params: Vec<&dyn ToSql> = chunk.iter().map(|id| id as &dyn ToSql).collect();
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(params))?;
        while let Some(row) = rows.next()? {
            result.insert(row.get(0)?);
        }
    }

    Ok(result)
}

/// find_includers 専用: resolved_file_id (完全一致) に加え、
/// resolved_file_id が NULL のエントリを base_filename_id でフォールバック検索する。
/// これにより同名ファイルが複数存在しても includer を正しく取得できる。
fn find_includer_file_ids(conn: &Connection, target_id: i64) -> anyhow::Result<HashSet<i64>> {
    let mut result: HashSet<i64> = HashSet::new();

    // (1) resolved_file_id による完全一致
    {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT fi.file_id FROM file_includes fi WHERE fi.resolved_file_id = ?"
        )?;
        let mut rows = stmt.query([target_id])?;
        while let Some(row) = rows.next()? {
            result.insert(row.get(0)?);
        }
    }

    // (2) resolved_file_id が NULL のエントリを base_filename_id で補完
    //     (同名ファイルが複数ある場合は偽陽性が出る可能性があるが、0件よりは有用)
    let filename_id: Option<i64> = conn
        .query_row("SELECT filename_id FROM files WHERE id = ?", [target_id], |r| r.get(0))
        .optional()?;
    if let Some(fn_id) = filename_id {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT fi.file_id FROM file_includes fi
             WHERE fi.resolved_file_id IS NULL AND fi.base_filename_id = ?"
        )?;
        let mut rows = stmt.query([fn_id])?;
        while let Some(row) = rows.next()? {
            result.insert(row.get(0)?);
        }
    }

    Ok(result)
}

fn get_file_paths_by_ids(conn: &Connection, ids: &[i64]) -> anyhow::Result<Vec<String>> {
    let mut results: Vec<String> = Vec::new();

    for chunk in ids.chunks(50) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "{} SELECT dp.full_path || '/' || sn.text
             FROM files f
             JOIN dir_paths dp ON f.directory_id = dp.id
             JOIN strings sn ON f.filename_id = sn.id
             WHERE f.id IN ({})",
            PATH_CTE, placeholders
        );
        let params: Vec<&dyn ToSql> = chunk.iter().map(|id| id as &dyn ToSql).collect();
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(params))?;
        while let Some(row) = rows.next()? {
            results.push(row.get(0)?);
        }
    }

    Ok(results)
}

/// 単語境界付きで行内のシンボルを検索し、見つかった場合は列オフセットを返す
fn find_word_in_line(line: &str, symbol: &str) -> Option<usize> {
    let sym_len = symbol.len();
    if sym_len == 0 {
        return None;
    }
    let bytes = line.as_bytes();
    let mut start = 0;

    while start + sym_len <= bytes.len() {
        if let Some(rel_pos) = line[start..].find(symbol) {
            let abs_pos = start + rel_pos;
            let is_word_char = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
            let before_ok = abs_pos == 0 || !is_word_char(bytes[abs_pos - 1]);
            let after_pos = abs_pos + sym_len;
            let after_ok = after_pos >= bytes.len() || !is_word_char(bytes[after_pos]);

            if before_ok && after_ok {
                return Some(abs_pos);
            }
            start = abs_pos + 1;
        } else {
            break;
        }
    }
    None
}

fn search_in_file(path: &str, symbol_name: &str, results: &mut Vec<Value>) {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return,
    };
    let reader = std::io::BufReader::new(file);

    for (line_idx, line_result) in reader.lines().enumerate() {
        if results.len() >= MAX_RESULTS {
            break;
        }
        let line = match line_result {
            Ok(l) => l,
            Err(_) => continue,
        };
        if let Some(col) = find_word_in_line(&line, symbol_name) {
            results.push(json!({
                "path": path,
                "line": line_idx + 1,
                "col": col,
                "context": line.trim(),
            }));
        }
    }
}

pub fn find_symbol_usages(
    conn: &Connection,
    symbol_name: &str,
    _current_file: Option<&str>,
) -> anyhow::Result<Value> {
    // 1. シンボルの定義ファイルIDを取得
    let def_ids = find_definition_file_ids(conn, symbol_name)?;
    let found_definition = !def_ids.is_empty();

    // 2. 定義ファイルをインクルードしているファイルのIDを取得
    let mut candidate_ids: HashSet<i64> = find_including_file_ids(conn, &def_ids)?;

    // 定義ファイル自身も対象に含める
    for id in &def_ids {
        candidate_ids.insert(*id);
    }

    // 上限を超える場合はトランケート
    let mut ids_vec: Vec<i64> = candidate_ids.into_iter().collect();
    if ids_vec.len() > MAX_FILES {
        ids_vec.truncate(MAX_FILES);
    }

    // 3. ファイルIDからパスを取得
    let file_paths = get_file_paths_by_ids(conn, &ids_vec)?;
    let searched_files = file_paths.len();

    // 4. 各ファイルでシンボルを単語境界付きで検索
    let mut results: Vec<Value> = Vec::new();
    for path in &file_paths {
        if results.len() >= MAX_RESULTS {
            break;
        }
        search_in_file(path, symbol_name, &mut results);
    }

    Ok(json!({
        "results": results,
        "searched_files": searched_files,
        "found_definition": found_definition,
    }))
}

/// .h / .hpp ファイルの ID セットに対して、対応する .cpp ペアの ID を収集する
fn find_cpp_peers_of_headers(conn: &Connection, ids: &HashSet<i64>) -> anyhow::Result<Vec<i64>> {
    let mut result = Vec::new();
    let ids_vec: Vec<i64> = ids.iter().cloned().collect();

    for chunk in ids_vec.chunks(50) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "{} SELECT dp.full_path, sn.text
             FROM files f
             JOIN dir_paths dp ON f.directory_id = dp.id
             JOIN strings sn ON f.filename_id = sn.id
             WHERE f.id IN ({}) AND (f.extension = 'h' OR f.extension = 'hpp')",
            PATH_CTE, placeholders
        );
        let params: Vec<&dyn ToSql> = chunk.iter().map(|id| id as &dyn ToSql).collect();
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(params))?;

        while let Some(row) = rows.next()? {
            let dir: String = row.get(0)?;
            let filename: String = row.get(1)?;
            let stem = std::path::Path::new(&filename)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let cpp_path = format!("{}/{}.cpp", dir, stem);

            let lookup_sql = format!(
                "{} SELECT f.id FROM files f
                 JOIN dir_paths dp ON f.directory_id = dp.id
                 JOIN strings sn ON f.filename_id = sn.id
                 WHERE dp.full_path || '/' || sn.text = ? LIMIT 1",
                PATH_CTE
            );
            if let Ok(Some(cpp_id)) = conn
                .query_row(&lookup_sql, [&cpp_path], |r| r.get::<_, i64>(0))
                .optional()
            {
                result.push(cpp_id);
            }
        }
    }
    Ok(result)
}

/// ファイル ID リストに対してパスとモジュール情報を取得する
fn get_file_paths_with_metadata(conn: &Connection, ids: &[i64]) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut results = Vec::new();

    for chunk in ids.chunks(50) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "{} SELECT dp.full_path || '/' || sn.text, sm.text, f.extension, rd.full_path
             FROM files f
             JOIN dir_paths dp ON f.directory_id = dp.id
             JOIN strings sn ON f.filename_id = sn.id
             LEFT JOIN modules m ON f.module_id = m.id
             LEFT JOIN strings sm ON m.name_id = sm.id
             LEFT JOIN dir_paths rd ON m.root_directory_id = rd.id
             WHERE f.id IN ({})",
            PATH_CTE, placeholders
        );
        let params: Vec<&dyn ToSql> = chunk.iter().map(|id| id as &dyn ToSql).collect();
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(params))?;
        while let Some(row) = rows.next()? {
            results.push(json!({
                "path":        row.get::<_, String>(0)?,
                "module_name": row.get::<_, Option<String>>(1)?,
                "extension":   row.get::<_, String>(2)?,
                "module_root": row.get::<_, Option<String>>(3)?,
            }));
        }
    }
    Ok(results)
}

/// 指定ファイルをインクルードしているファイルの一覧を返す（include 逆引き）。
///
/// - `.cpp` を渡した場合は同ディレクトリの対応 `.h` をターゲットとして使用する。
///   `.h` が DB に存在しない場合は `.cpp` 自体を試みる。
/// - 結果に `.h` / `.hpp` ファイルが含まれる場合、対応する `.cpp` ペアも自動追加する。
pub fn find_includers(conn: &Connection, file_path: &str) -> anyhow::Result<Value> {
    let normalized = file_path.replace('\\', "/");
    let path_obj = std::path::Path::new(&normalized);
    let ext = path_obj
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    // ターゲットパスを決定: .cpp の場合は .h ペアをターゲットにする
    let candidate_target = if ext == "cpp" {
        let stem = path_obj.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let dir = path_obj
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .replace('\\', "/");
        to_db_path_format(&format!("{}/{}.h", dir, stem))
    } else {
        to_db_path_format(&normalized)
    };

    let path_sql = format!(
        "{} SELECT f.id FROM files f
         JOIN dir_paths dp ON f.directory_id = dp.id
         JOIN strings sn ON f.filename_id = sn.id
         WHERE dp.full_path || '/' || sn.text = ? LIMIT 1",
        PATH_CTE
    );

    // まず candidate_target で検索、見つからなければ元のパスで再試行
    let (target_id, resolved_target_path) = {
        let id: Option<i64> = conn
            .query_row(&path_sql, [&candidate_target], |r| r.get(0))
            .optional()?;
        if let Some(i) = id {
            (i, candidate_target.clone())
        } else if ext == "cpp" && candidate_target != to_db_path_format(&normalized) {
            let orig_db = to_db_path_format(&normalized);
            let orig_id: Option<i64> = conn
                .query_row(&path_sql, [&orig_db], |r| r.get(0))
                .optional()?;
            match orig_id {
                Some(i) => (i, orig_db),
                None => {
                    return Ok(json!({
                        "files": [],
                        "found_target": false,
                        "target_path": normalized
                    }))
                }
            }
        } else {
            return Ok(json!({
                "files": [],
                "found_target": false,
                "target_path": normalized
            }));
        }
    };

    // ターゲットをインクルードしているファイル ID を取得
    // resolved_file_id が NULL のエントリも base_filename_id で補完する
    let including_ids = find_includer_file_ids(conn, target_id)?;

    if including_ids.is_empty() {
        return Ok(json!({
            "files": [],
            "found_target": true,
            "target_path": resolved_target_path
        }));
    }

    // .h インクルーダーに対応する .cpp ペアも追加
    let mut all_ids: HashSet<i64> = including_ids.clone();
    let cpp_peers = find_cpp_peers_of_headers(conn, &including_ids)?;
    for id in cpp_peers {
        all_ids.insert(id);
    }

    let ids_vec: Vec<i64> = all_ids.into_iter().collect();
    let files = get_file_paths_with_metadata(conn, &ids_vec)?;

    Ok(json!({
        "files": files,
        "found_target": true,
        "target_path": resolved_target_path
    }))
}

const STREAM_BATCH_SIZE: usize = 15;

/// ファイルを STREAM_BATCH_SIZE ごとにバッチ通知するストリーミング版
pub fn find_symbol_usages_async<F>(
    conn: &Connection,
    symbol_name: &str,
    _current_file: Option<&str>,
    mut on_items: F,
) -> anyhow::Result<Value>
where
    F: FnMut(Vec<Value>) -> anyhow::Result<()>,
{
    let def_ids = find_definition_file_ids(conn, symbol_name)?;
    let found_definition = !def_ids.is_empty();

    let mut candidate_ids: HashSet<i64> = find_including_file_ids(conn, &def_ids)?;
    for id in &def_ids {
        candidate_ids.insert(*id);
    }

    let mut ids_vec: Vec<i64> = candidate_ids.into_iter().collect();
    if ids_vec.len() > MAX_FILES {
        ids_vec.truncate(MAX_FILES);
    }

    let file_paths = get_file_paths_by_ids(conn, &ids_vec)?;
    let searched_files = file_paths.len();

    let mut total_results = 0;
    let mut batch: Vec<Value> = Vec::new();

    for path in &file_paths {
        if total_results >= MAX_RESULTS {
            break;
        }
        search_in_file(path, symbol_name, &mut batch);

        if batch.len() >= STREAM_BATCH_SIZE {
            total_results += batch.len();
            on_items(batch)?;
            batch = Vec::new();
        }
    }

    // 残りを送信
    if !batch.is_empty() {
        on_items(batch)?;
    }

    Ok(json!({
        "searched_files": searched_files,
        "found_definition": found_definition,
    }))
}

/// find_includers のストリーミング版。ファイルを STREAM_BATCH_SIZE ごとにバッチ通知する。
pub fn find_includers_async<F>(
    conn: &Connection,
    file_path: &str,
    mut on_items: F,
) -> anyhow::Result<Value>
where
    F: FnMut(Vec<Value>) -> anyhow::Result<()>,
{
    let normalized = file_path.replace('\\', "/");
    let path_obj = std::path::Path::new(&normalized);
    let ext = path_obj
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let candidate_target = if ext == "cpp" {
        let stem = path_obj.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let dir = path_obj
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .replace('\\', "/");
        to_db_path_format(&format!("{}/{}.h", dir, stem))
    } else {
        to_db_path_format(&normalized)
    };

    let path_sql = format!(
        "{} SELECT f.id FROM files f
         JOIN dir_paths dp ON f.directory_id = dp.id
         JOIN strings sn ON f.filename_id = sn.id
         WHERE dp.full_path || '/' || sn.text = ? LIMIT 1",
        PATH_CTE
    );

    let (target_id, resolved_target_path) = {
        let id: Option<i64> = conn
            .query_row(&path_sql, [&candidate_target], |r| r.get(0))
            .optional()?;
        if let Some(i) = id {
            (i, candidate_target.clone())
        } else if ext == "cpp" && candidate_target != to_db_path_format(&normalized) {
            let orig_db = to_db_path_format(&normalized);
            let orig_id: Option<i64> = conn
                .query_row(&path_sql, [&orig_db], |r| r.get(0))
                .optional()?;
            match orig_id {
                Some(i) => (i, orig_db),
                None => {
                    return Ok(json!({
                        "found_target": false,
                        "target_path": normalized,
                        "total_files": 0
                    }))
                }
            }
        } else {
            return Ok(json!({
                "found_target": false,
                "target_path": normalized,
                "total_files": 0
            }));
        }
    };

    let including_ids = find_includer_file_ids(conn, target_id)?;

    if including_ids.is_empty() {
        return Ok(json!({
            "found_target": true,
            "target_path": resolved_target_path,
            "total_files": 0
        }));
    }

    let mut all_ids: HashSet<i64> = including_ids.clone();
    let cpp_peers = find_cpp_peers_of_headers(conn, &including_ids)?;
    for id in cpp_peers {
        all_ids.insert(id);
    }

    let ids_vec: Vec<i64> = all_ids.into_iter().collect();
    let total_files = ids_vec.len();
    let all_files = get_file_paths_with_metadata(conn, &ids_vec)?;

    // STREAM_BATCH_SIZE ごとに通知
    for chunk in all_files.chunks(STREAM_BATCH_SIZE) {
        on_items(chunk.to_vec())?;
    }

    Ok(json!({
        "found_target": true,
        "target_path": resolved_target_path,
        "total_files": total_files
    }))
}
