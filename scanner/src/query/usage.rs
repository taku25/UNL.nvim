use rusqlite::Connection;
use rusqlite::types::ToSql;
use rusqlite::OptionalExtension;
use serde_json::{json, Value};
use std::collections::HashSet;
use tree_sitter::Parser;
use crate::db::path::{PATH_CTE, to_db_path_format};

const MAX_RESULTS: usize = 300;
const STREAM_BATCH_SIZE: usize = 15;

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

/// tree-sitter でファイルをパースし、class_name が型として参照されている箇所を収集する。
/// `is_extra` ノード（コメント等）は除外し、クラス/構造体の定義宣言名も除外する。
/// 戻り値: Vec<(line_1based, col_0based, context_line_trimmed)>
fn find_type_refs_in_file(path: &str, class_name: &str) -> Vec<(u32, u32, String)> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let language: tree_sitter::Language = tree_sitter_unreal_cpp::LANGUAGE.into();
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return vec![];
    }
    let tree = match parser.parse(&content, None) {
        Some(t) => t,
        None => return vec![],
    };
    let src = content.as_bytes();
    let lines: Vec<&str> = content.lines().collect();
    let mut results = Vec::new();
    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    collect_type_refs(tree.root_node(), src, &lines, class_name, &mut results, &mut seen);
    results
}

fn collect_type_refs(
    node: tree_sitter::Node,
    src: &[u8],
    lines: &[&str],
    class_name: &str,
    results: &mut Vec<(u32, u32, String)>,
    seen: &mut HashSet<(u32, u32)>,
) {
    // コメント・文字列リテラル等の extra ノードはスキップ
    if node.is_extra() {
        return;
    }

    match node.kind() {
        "type_identifier" => {
            let text = std::str::from_utf8(&src[node.byte_range()]).unwrap_or("");
            if text == class_name {
                // クラス/構造体の定義宣言名は除外（使用箇所ではないため）
                let is_def = node.parent().map(|p| {
                    matches!(
                        p.kind(),
                        "class_specifier" | "struct_specifier"
                            | "unreal_class_declaration" | "unreal_struct_declaration"
                    ) && p.child_by_field_name("name")
                        .map(|n| n.id() == node.id())
                        .unwrap_or(false)
                }).unwrap_or(false);

                if !is_def {
                    let pos = node.start_position();
                    if seen.insert((pos.row as u32, pos.column as u32)) {
                        let ctx = lines.get(pos.row)
                            .map(|l| l.trim().to_string())
                            .unwrap_or_default();
                        results.push((pos.row as u32 + 1, pos.column as u32, ctx));
                    }
                }
            }
        }
        "namespace_identifier" => {
            // qualified_identifier のスコープ部分: AMyActor::StaticClass() など
            if let Some(parent) = node.parent() {
                if parent.kind() == "qualified_identifier" {
                    if let Some(scope) = parent.child_by_field_name("scope") {
                        if scope.id() == node.id() {
                            let text = std::str::from_utf8(&src[node.byte_range()]).unwrap_or("");
                            if text == class_name {
                                let pos = node.start_position();
                                if seen.insert((pos.row as u32, pos.column as u32)) {
                                    let ctx = lines.get(pos.row)
                                        .map(|l| l.trim().to_string())
                                        .unwrap_or_default();
                                    results.push((pos.row as u32 + 1, pos.column as u32, ctx));
                                }
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    for child in children {
        collect_type_refs(child, src, lines, class_name, results, seen);
    }
}

/// tree-sitter でファイルをパースし、class_name::method_name のメソッド呼び出し箇所を収集する。
/// カバーするパターン:
///   - `obj.method()` / `obj->method()`  → field_expression の field_identifier
///   - `ClassName::method()`              → qualified_identifier (scope == class_name, name == method_name)
fn find_method_refs_in_file(path: &str, class_name: &str, method_name: &str) -> Vec<(u32, u32, String)> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let language: tree_sitter::Language = tree_sitter_unreal_cpp::LANGUAGE.into();
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return vec![];
    }
    let tree = match parser.parse(&content, None) {
        Some(t) => t,
        None => return vec![],
    };
    let src = content.as_bytes();
    let lines: Vec<&str> = content.lines().collect();
    let mut results = Vec::new();
    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    collect_method_refs(tree.root_node(), src, &lines, class_name, method_name, &mut results, &mut seen);
    results
}

fn collect_method_refs(
    node: tree_sitter::Node,
    src: &[u8],
    lines: &[&str],
    class_name: &str,
    method_name: &str,
    results: &mut Vec<(u32, u32, String)>,
    seen: &mut HashSet<(u32, u32)>,
) {
    if node.is_extra() {
        return;
    }

    match node.kind() {
        // obj->method() / obj.method() → field_expression の field_identifier
        "field_identifier" => {
            let text = std::str::from_utf8(&src[node.byte_range()]).unwrap_or("");
            if text == method_name {
                if let Some(parent) = node.parent() {
                    if parent.kind() == "field_expression" {
                        let pos = node.start_position();
                        if seen.insert((pos.row as u32, pos.column as u32)) {
                            let ctx = lines.get(pos.row)
                                .map(|l| l.trim().to_string())
                                .unwrap_or_default();
                            results.push((pos.row as u32 + 1, pos.column as u32, ctx));
                        }
                    }
                }
            }
        }
        // ClassName::method() → qualified_identifier の name (identifier)
        "identifier" => {
            let text = std::str::from_utf8(&src[node.byte_range()]).unwrap_or("");
            if text == method_name {
                if let Some(parent) = node.parent() {
                    if parent.kind() == "qualified_identifier" {
                        // name フィールドが自分自身であること
                        if let Some(name_field) = parent.child_by_field_name("name") {
                            if name_field.id() == node.id() {
                                // scope が class_name と一致すること
                                if let Some(scope_node) = parent.child_by_field_name("scope") {
                                    let scope_text = std::str::from_utf8(&src[scope_node.byte_range()]).unwrap_or("");
                                    if scope_text == class_name {
                                        let pos = node.start_position();
                                        if seen.insert((pos.row as u32, pos.column as u32)) {
                                            let ctx = lines.get(pos.row)
                                                .map(|l| l.trim().to_string())
                                                .unwrap_or_default();
                                            results.push((pos.row as u32 + 1, pos.column as u32, ctx));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    for child in children {
        collect_method_refs(child, src, lines, class_name, method_name, results, seen);
    }
}


/// - `header_path` が Some (DB形式パス) → find_includers ベースのスコープ
/// - `header_path` が None → symbol_name でクラス定義を DB 検索してフォールバック
fn resolve_search_scope(
    conn: &Connection,
    symbol_name: &str,
    header_path: Option<&str>,
) -> anyhow::Result<(Vec<Value>, bool)> {
    if let Some(header) = header_path {
        let db_path = to_db_path_format(&header.replace('\\', "/"));
        let path_sql = format!(
            "{} SELECT f.id FROM files f
             JOIN dir_paths dp ON f.directory_id = dp.id
             JOIN strings sn ON f.filename_id = sn.id
             WHERE dp.full_path || '/' || sn.text = ? LIMIT 1",
            PATH_CTE
        );
        let target_id: Option<i64> = conn
            .query_row(&path_sql, [&db_path], |r| r.get(0))
            .optional()?;

        if let Some(tid) = target_id {
            let including_ids = find_includer_file_ids(conn, tid)?;
            let mut all_ids: HashSet<i64> = including_ids.clone();
            all_ids.insert(tid); // ヘッダー自身も検索対象に含める
            let cpp_peers = find_cpp_peers_of_headers(conn, &including_ids)?;
            for id in cpp_peers {
                all_ids.insert(id);
            }
            let ids_vec: Vec<i64> = all_ids.into_iter().collect();
            let files = get_file_paths_with_metadata(conn, &ids_vec)?;
            return Ok((files, true));
        }
        // header が DB に見つからない場合はフォールバックへ
    }

    // フォールバック: symbol_name でクラス定義ファイルを DB から取得
    let def_ids = find_definition_file_ids(conn, symbol_name)?;
    let found = !def_ids.is_empty();
    let mut candidate_ids = find_including_file_ids(conn, &def_ids)?;
    for id in &def_ids {
        candidate_ids.insert(*id);
    }
    let ids_vec: Vec<i64> = candidate_ids.into_iter().collect();
    let files = get_file_paths_with_metadata(conn, &ids_vec)?;
    Ok((files, found))
}

/// tree-sitter でシンボル使用箇所を検索する（同期版）。
/// - `symbol_name`: 検索スコープのクラス名 (例: "AMyActor")
/// - `file_path`: クラスのヘッダーファイルパス (DB形式、省略時は DB フォールバック)
/// - `method_name`: Some → メソッド参照検索モード、None → 型参照検索モード
pub fn find_symbol_usages(
    conn: &Connection,
    symbol_name: &str,
    file_path: Option<&str>,
    method_name: Option<&str>,
) -> anyhow::Result<Value> {
    let (files_meta, found_definition) = resolve_search_scope(conn, symbol_name, file_path)?;
    let searched_files = files_meta.len();
    let mut results: Vec<Value> = Vec::new();

    'outer: for item in &files_meta {
        let path = item["path"].as_str().unwrap_or("");
        let module_name = item["module_name"].as_str().unwrap_or("").to_string();
        let module_root = item["module_root"].as_str().unwrap_or("").to_string();

        let refs = if let Some(method) = method_name {
            find_method_refs_in_file(path, symbol_name, method)
        } else {
            find_type_refs_in_file(path, symbol_name)
        };
        for (line, col, context) in refs {
            results.push(json!({
                "path":        path,
                "module_name": module_name,
                "module_root": module_root,
                "line":        line,
                "col":         col,
                "context":     context,
            }));
            if results.len() >= MAX_RESULTS {
                break 'outer;
            }
        }
    }

    Ok(json!({
        "results":          results,
        "searched_files":   searched_files,
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

/// tree-sitter でシンボル使用箇所を検索するストリーミング版。
/// 結果を STREAM_BATCH_SIZE ごとにバッチ通知する。
/// - `method_name`: Some → メソッド参照検索モード、None → 型参照検索モード
pub fn find_symbol_usages_async<F>(
    conn: &Connection,
    symbol_name: &str,
    file_path: Option<&str>,
    method_name: Option<&str>,
    mut on_items: F,
) -> anyhow::Result<Value>
where
    F: FnMut(Vec<Value>) -> anyhow::Result<()>,
{
    let (files_meta, found_definition) = resolve_search_scope(conn, symbol_name, file_path)?;
    let searched_files = files_meta.len();

    let mut total_results = 0usize;
    let mut batch: Vec<Value> = Vec::new();

    'outer: for item in &files_meta {
        let path = item["path"].as_str().unwrap_or("");
        let module_name = item["module_name"].as_str().unwrap_or("").to_string();
        let module_root = item["module_root"].as_str().unwrap_or("").to_string();

        let refs = if let Some(method) = method_name {
            find_method_refs_in_file(path, symbol_name, method)
        } else {
            find_type_refs_in_file(path, symbol_name)
        };
        for (line, col, context) in refs {
            batch.push(json!({
                "path":        path,
                "module_name": module_name,
                "module_root": module_root,
                "line":        line,
                "col":         col,
                "context":     context,
            }));
            total_results += 1;
            if batch.len() >= STREAM_BATCH_SIZE {
                on_items(std::mem::take(&mut batch))?;
            }
            if total_results >= MAX_RESULTS {
                break 'outer;
            }
        }
    }

    if !batch.is_empty() {
        on_items(batch)?;
    }

    Ok(json!({
        "searched_files":   searched_files,
        "found_definition": found_definition,
        "total_results":    total_results,
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
