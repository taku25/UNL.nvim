use rusqlite::Connection;
use rusqlite::types::ToSql;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::BufRead;
use crate::db::path::PATH_CTE;

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
