use std::collections::{HashMap, HashSet};
use rusqlite::Connection;
use serde_json::{json, Value};
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};
use crate::db::path::{PATH_CTE, to_db_path_format};
use super::include::compute_include_path;

/// バッファ内の #include ステートメントを解析して返す
struct ExistingInclude {
    path: String,   // e.g. "GameFramework/Actor.h"
    line: u32,      // 1-based
    #[allow(dead_code)]
    is_system: bool, // <...> 形式
}

/// ファイル内で使われている型名を Tree-sitter で収集し、
/// DBと照合して不足している #include を返す。
/// トランジティブインクルード（既存 include が推移的にカバーしているもの）は除外。
pub fn check_includes(
    conn: &Connection,
    file_path: &str,
    content: &str,
) -> anyhow::Result<Value> {
    let language: tree_sitter::Language = tree_sitter_unreal_cpp::LANGUAGE.into();
    let mut parser = Parser::new();
    parser.set_language(&language)?;
    let tree = parser.parse(content, None)
        .ok_or_else(|| anyhow::anyhow!("parse failed"))?;
    let root = tree.root_node();

    // 1. 既存の #include を抽出
    let include_query_str =
        "(preproc_include path: [(string_literal) @path (system_lib_string) @sys]) @include";
    let include_query = Query::new(&language, include_query_str)?;
    let mut inc_cursor = QueryCursor::new();
    let mut inc_matches = inc_cursor.matches(&include_query, root, content.as_bytes());

    let mut existing_includes: Vec<ExistingInclude> = Vec::new();
    let mut last_include_line: u32 = 0;

    while let Some(m) = inc_matches.next() {
        let mut path_text = String::new();
        let mut is_system = false;
        let mut include_line = 0u32;
        for cap in m.captures {
            let cname = include_query.capture_names()[cap.index as usize];
            let text = cap.node.utf8_text(content.as_bytes()).unwrap_or("").to_string();
            match cname {
                "path" => {
                    path_text = text.trim_matches('"').to_string();
                    is_system = false;
                }
                "sys" => {
                    path_text = text.trim_matches('<').trim_matches('>').to_string();
                    is_system = true;
                }
                "include" => {
                    include_line = cap.node.start_position().row as u32 + 1;
                }
                _ => {}
            }
        }
        if !path_text.is_empty() {
            last_include_line = last_include_line.max(include_line);
            existing_includes.push(ExistingInclude {
                path: path_text,
                line: include_line,
                is_system,
            });
        }
    }

    // 直接インクルードのパスセット（大文字小文字無視、正規化済み）
    let existing_paths: HashSet<String> = existing_includes.iter()
        .map(|i| normalize_include_path(&i.path))
        .collect();

    // 2. ファイル内で使われている型名 (type_identifier) を収集
    let type_query_str = "(type_identifier) @type";
    let type_query = Query::new(&language, type_query_str)?;
    let mut type_cursor = QueryCursor::new();
    let mut type_matches = type_cursor.matches(&type_query, root, content.as_bytes());

    // 型名 → 最初に登場した行番号
    let mut type_usages: HashMap<String, u32> = HashMap::new();
    while let Some(m) = type_matches.next() {
        for cap in m.captures {
            let name = cap.node.utf8_text(content.as_bytes()).unwrap_or("").to_string();
            let line = cap.node.start_position().row as u32 + 1;
            type_usages.entry(name).or_insert(line);
        }
    }

    if type_usages.is_empty() {
        return Ok(json!({ "missing": [], "insert_line": last_include_line }));
    }

    // 3. 現在ファイルの file_id / module_id / module_root を取得
    let db_path = to_db_path_format(file_path);
    let (current_file_id, module_id, module_root): (i64, i64, String) = {
        let sql = format!(
            "{}
            SELECT f.id, f.module_id, mroot.full_path
            FROM files f
            JOIN modules m ON f.module_id = m.id
            JOIN dir_paths mroot ON m.root_directory_id = mroot.id
            JOIN dir_paths dp ON f.directory_id = dp.id
            JOIN strings sn ON f.filename_id = sn.id
            WHERE dp.full_path || '/' || sn.text = ?
            LIMIT 1",
            PATH_CTE
        );
        match conn.query_row(&sql, [&db_path], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))
        }) {
            Ok(v) => v,
            Err(_) => return Ok(json!({ "missing": [], "insert_line": last_include_line })),
        }
    };

    // 4. 依存モジュールの root_path マップを構築
    let deep_deps_json: Option<String> = conn.query_row(
        "SELECT deep_dependencies FROM modules WHERE id = ?",
        [module_id],
        |row| row.get(0),
    ).ok().flatten();

    let mut module_roots: Vec<(i64, String)> = vec![(module_id, module_root)];
    if let Some(json_str) = &deep_deps_json {
        if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(json_str) {
            let dep_names: Vec<String> = arr.into_iter()
                .filter_map(|v| if let Value::String(s) = v { Some(s) } else { None })
                .collect();
            if !dep_names.is_empty() {
                let placeholders = dep_names.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
                let sql = format!(
                    "{}
                    SELECT m.id, dp.full_path
                    FROM modules m
                    JOIN strings sm ON m.name_id = sm.id
                    JOIN dir_paths dp ON m.root_directory_id = dp.id
                    WHERE sm.text IN ({})",
                    PATH_CTE, placeholders
                );
                let mut stmt = conn.prepare(&sql)?;
                let params: Vec<&dyn rusqlite::ToSql> = dep_names.iter()
                    .map(|s| s as &dyn rusqlite::ToSql)
                    .collect();
                let mut rows = stmt.query(params.as_slice())?;
                while let Some(row) = rows.next()? {
                    let mid: i64 = row.get(0)?;
                    let root: String = row.get(1)?;
                    if mid != module_id {
                        module_roots.push((mid, root));
                    }
                }
            }
        }
    }

    let root_map: HashMap<i64, String> = module_roots.into_iter().collect();
    let module_ids_str = root_map.keys()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(", ");

    // 5. 既存インクルードから「推移的に到達可能な全ファイルID」を求める
    //    (CoreMinimal.h が間接的にカバーしているものをすべて除外するため)
    let existing_base_names: Vec<String> = existing_includes.iter()
        .map(|inc| inc.path.split('/').next_back().unwrap_or(&inc.path).to_string())
        .collect();
    let reachable_file_ids = get_transitive_reachable_file_ids(conn, &existing_base_names)?;

    // 6. 使用された型名を DB で一括検索 → ヘッダーファイル + file_id を特定
    let type_names: Vec<String> = type_usages.keys().cloned().collect();
    let placeholders = type_names.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let sql = format!(
        "{}
        SELECT sc.text, dp.full_path || '/' || sn.text, f.module_id, f.id
        FROM classes c
        JOIN strings sc ON c.name_id = sc.id
        JOIN files f ON c.file_id = f.id
        JOIN strings sn ON f.filename_id = sn.id
        JOIN dir_paths dp ON f.directory_id = dp.id
        WHERE sc.text IN ({})
          AND f.is_header = 1
          AND f.module_id IN ({})
          AND f.id != ?",
        PATH_CTE, placeholders, module_ids_str
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut all_params: Vec<Box<dyn rusqlite::ToSql>> = type_names.iter()
        .map(|s| Box::new(s.clone()) as Box<dyn rusqlite::ToSql>)
        .collect();
    all_params.push(Box::new(current_file_id));
    let params_ref: Vec<&dyn rusqlite::ToSql> = all_params.iter()
        .map(|b| b.as_ref())
        .collect();

    // 型名 → 候補リスト (インクルードパス, file_id)
    // 同名クラスが複数のヘッダーに存在する場合（例: TArray は Core/Array.h と
    // TraceLog/standalone_prologue.h の両方に定義）、全候補を保持してから
    // ステップ7でまとめて判定する。一つでもカバー済みなら "missing" から除外する。
    let mut required: HashMap<String, Vec<(String, i64)>> = HashMap::new();
    let mut rows = stmt.query(params_ref.as_slice())?;
    while let Some(row) = rows.next()? {
        let class_name: String = row.get(0)?;
        let full_path: String = row.get(1)?;
        let mid: i64 = row.get(2)?;
        let fid: i64 = row.get(3)?;

        let include_path = if let Some(root) = root_map.get(&mid) {
            compute_include_path(&full_path, root)
        } else {
            continue;
        };

        if !include_path.is_empty() {
            required.entry(class_name).or_default().push((include_path, fid));
        }
    }

    // 7. 不足インクルードを計算
    //    - いずれかの候補が直接インクルード済み (existing_paths に一致) → スキップ
    //    - いずれかの候補が推移的にカバー済み (reachable_file_ids に file_id が含まれる) → スキップ
    //    - 上記どちらでもない場合は "最良候補" を missing として報告する
    //      (Public/ を含むパスを優先し、次に短いインクルードパスを優先)
    let mut missing: Vec<Value> = Vec::new();
    for (type_name, candidates) in &required {
        // いずれかの候補が既にカバーされているか
        let any_covered = candidates.iter().any(|(header, fid)| {
            existing_paths.contains(&normalize_include_path(header))
                || reachable_file_ids.contains(fid)
        });
        if any_covered {
            continue;
        }

        // カバーされていない場合は最良候補を選ぶ
        // 優先度: Public/ を含む > include_path が短い
        let best = candidates.iter().min_by(|(a, _), (b, _)| {
            let a_pub = a.contains("Public/") as u8;
            let b_pub = b.contains("Public/") as u8;
            b_pub.cmp(&a_pub).then_with(|| a.len().cmp(&b.len()))
        });
        if let Some((header, _)) = best {
            let line = type_usages.get(type_name).copied().unwrap_or(0);
            missing.push(json!({
                "symbol": type_name,
                "header": header,
                "line": line,
            }));
        }
    }

    // シンボル名でソート
    missing.sort_by(|a, b| {
        a["symbol"].as_str().unwrap_or("").cmp(b["symbol"].as_str().unwrap_or(""))
    });

    // 同一ヘッダーの重複除去
    let mut seen_headers: HashSet<String> = HashSet::new();
    let missing_deduped: Vec<Value> = missing.into_iter().filter(|item| {
        let h = item["header"].as_str().unwrap_or("").to_string();
        seen_headers.insert(h)
    }).collect();

    // 挿入位置: .generated.h の直前 or 最後の #include の次
    let generated_line = existing_includes.iter()
        .find(|i| i.path.contains(".generated.h"))
        .map(|i| i.line);
    let insert_line = if let Some(gen) = generated_line {
        gen.saturating_sub(1).max(1)
    } else if last_include_line > 0 {
        last_include_line + 1
    } else {
        1
    };

    Ok(json!({
        "missing": missing_deduped,
        "insert_line": insert_line,
    }))
}

/// 既存インクルードのベースファイル名から、
/// file_includes テーブルを再帰的に辿って到達可能な全ファイル ID を返す。
fn get_transitive_reachable_file_ids(
    conn: &Connection,
    base_filenames: &[String],
) -> anyhow::Result<HashSet<i64>> {
    if base_filenames.is_empty() {
        return Ok(HashSet::new());
    }

    // ステップ1: ベースファイル名 → file_id（同名ファイルが複数あれば全て含める）
    let placeholders = base_filenames.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let seed_sql = format!(
        "SELECT DISTINCT f.id FROM files f
         JOIN strings s ON f.filename_id = s.id
         WHERE s.text IN ({}) AND f.is_header = 1",
        placeholders
    );
    let mut stmt = conn.prepare(&seed_sql)?;
    let params: Vec<&dyn rusqlite::ToSql> = base_filenames.iter()
        .map(|s| s as &dyn rusqlite::ToSql)
        .collect();
    let mut rows = stmt.query(params.as_slice())?;
    let mut seed_ids: Vec<i64> = Vec::new();
    while let Some(row) = rows.next()? {
        seed_ids.push(row.get(0)?);
    }

    if seed_ids.is_empty() {
        return Ok(HashSet::new());
    }

    // ステップ2: WITH RECURSIVE で推移的インクルードを展開
    // - resolved_file_id が設定済みの場合はそれを使用（正確）
    // - NULL の場合は base_filename_id で同名ファイルを全て含める（保守的だが false positive 抑制に有効）
    let seed_str = seed_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(", ");
    let recursive_sql = format!(
        "WITH RECURSIVE reachable(fid) AS (
            SELECT id FROM files WHERE id IN ({seed})
            UNION
            SELECT fi.resolved_file_id
            FROM file_includes fi
            JOIN reachable r ON fi.file_id = r.fid
            WHERE fi.resolved_file_id IS NOT NULL
            UNION
            SELECT f2.id
            FROM file_includes fi2
            JOIN reachable r2 ON fi2.file_id = r2.fid
            JOIN files f2 ON f2.filename_id = fi2.base_filename_id
            WHERE fi2.resolved_file_id IS NULL AND f2.is_header = 1
        )
        SELECT DISTINCT fid FROM reachable",
        seed = seed_str
    );

    let mut stmt2 = conn.prepare(&recursive_sql)?;
    let mut rows2 = stmt2.query([])?;
    let mut result = HashSet::new();
    while let Some(row) = rows2.next()? {
        result.insert(row.get::<_, i64>(0)?);
    }

    Ok(result)
}

/// インクルードパスの正規化（大文字小文字無視 + スラッシュ統一）
fn normalize_include_path(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}
