use rusqlite::Connection;
use serde_json::{json, Value};
use crate::db::path::{PATH_CTE, to_db_path_format};

/// #include パス補完: 現在ファイルのモジュール + その依存モジュールの
/// ヘッダーファイルを検索し、インクルードパス形式で返す
pub fn get_include_completions(
    conn: &Connection,
    file_path: &str,
    prefix: &str,
) -> anyhow::Result<Value> {
    let db_path = to_db_path_format(file_path);

    // 1. 現在ファイルの module_id を取得
    let module_id: Option<i64> = {
        let sql = format!(
            "{}
            SELECT f.module_id
            FROM files f
            JOIN dir_paths dp ON f.directory_id = dp.id
            JOIN strings sn ON f.filename_id = sn.id
            WHERE dp.full_path || '/' || sn.text = ?
            LIMIT 1",
            PATH_CTE
        );
        conn.query_row(&sql, [&db_path], |row| row.get(0)).ok()
    };

    let Some(module_id) = module_id else {
        return Ok(json!([]));
    };

    // 2. 現在モジュールの deep_dependencies (JSON) + モジュール名 を取得
    let (module_root, deep_deps_json): (String, Option<String>) = {
        let sql = format!(
            "{}
            SELECT dp.full_path, m.deep_dependencies
            FROM modules m
            JOIN dir_paths dp ON m.root_directory_id = dp.id
            WHERE m.id = ?
            LIMIT 1",
            PATH_CTE
        );
        conn.query_row(&sql, [module_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?
    };

    // 3. 依存モジュール名リストを構築（自身も含む）
    let mut dep_names: Vec<String> = vec![];
    if let Some(json_str) = &deep_deps_json {
        if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(json_str) {
            for v in arr {
                if let Value::String(s) = v {
                    dep_names.push(s);
                }
            }
        }
    }

    // 4. 依存モジュール全ての module_id + root_path を取得
    //    自身のモジュールは既知なので含める
    let mut module_roots: Vec<(i64, String)> = vec![(module_id, module_root)];

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
            // 自身は既に追加済みなのでスキップ
            if mid != module_id {
                module_roots.push((mid, root));
            }
        }
    }

    // 5. それらモジュールのヘッダーファイルを取得し、インクルードパスを計算
    let prefix_lower = prefix.to_lowercase();
    let module_ids_str = module_roots.iter()
        .map(|(id, _)| id.to_string())
        .collect::<Vec<_>>()
        .join(", ");

    let sql = format!(
        "{}
        SELECT dp.full_path || '/' || sn.text, f.module_id
        FROM files f
        JOIN dir_paths dp ON f.directory_id = dp.id
        JOIN strings sn ON f.filename_id = sn.id
        WHERE f.is_header = 1
          AND f.module_id IN ({})",
        PATH_CTE, module_ids_str
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;

    // module_id → root_path マップ
    let root_map: std::collections::HashMap<i64, &str> = module_roots.iter()
        .map(|(id, root)| (*id, root.as_str()))
        .collect();

    let mut results: Vec<Value> = Vec::new();
    while let Some(row) = rows.next()? {
        let full_path: String = row.get(0)?;
        let mid: i64 = row.get(1)?;

        let include_path = if let Some(root) = root_map.get(&mid) {
            compute_include_path(&full_path, root)
        } else {
            continue;
        };

        if include_path.is_empty() {
            continue;
        }

        // prefix フィルター（大文字小文字無視）
        if !prefix.is_empty() && !include_path.to_lowercase().contains(&prefix_lower) {
            continue;
        }

        results.push(json!({
            "label": include_path,
            "full_path": full_path,
        }));
    }

    // ラベルでソートして返す
    results.sort_by(|a, b| {
        let la = a["label"].as_str().unwrap_or("");
        let lb = b["label"].as_str().unwrap_or("");
        la.cmp(lb)
    });

    Ok(json!(results))
}

/// ファイルのフルパスからモジュールルートを除いたインクルードパスを計算する。
/// `Public/`, `Classes/`, `Private/` などの中間ディレクトリを除く。
fn compute_include_path(full_path: &str, module_root: &str) -> String {
    // パスを正規化（バックスラッシュ → スラッシュ）
    let full = full_path.replace('\\', "/");
    let root = module_root.replace('\\', "/");

    // module_root を除いた相対パス
    let relative = if let Some(rel) = full.strip_prefix(&root) {
        rel.trim_start_matches('/')
    } else {
        return String::new();
    };

    // Public/, Classes/, Private/, Internal/ などを剥がす
    let stripped = strip_visibility_prefix(relative);

    stripped.to_string()
}

/// `Public/GameFramework/Actor.h` → `GameFramework/Actor.h`
/// `Private/Foo.h` → skip (privateヘッダーは公開しない)
fn strip_visibility_prefix(relative: &str) -> &str {
    // 先頭のセグメントを確認
    let public_prefixes = ["Public/", "Classes/", "Interfaces/"];
    let private_prefixes = ["Private/", "Internal/"];

    for prefix in &public_prefixes {
        if let Some(rest) = relative.strip_prefix(prefix) {
            return rest;
        }
    }
    // Private ヘッダーはインクルードパスとして提供しない → 空文字で除外
    for prefix in &private_prefixes {
        if relative.starts_with(prefix) {
            return "";
        }
    }

    // どのプレフィックスもない場合（モジュールルート直下）はそのまま
    relative
}
