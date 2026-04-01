use rusqlite::{params, Connection, ToSql, OptionalExtension};
use serde_json::{json, Value};
use std::collections::HashMap;

pub fn find_derived_classes(conn: &Connection, base_class: String) -> anyhow::Result<Value> {
    let mut stmt = conn.prepare(
        "SELECT sc.text as name, '' as base_class, sp.text as path, sm.text as module_name
         FROM classes c
         JOIN strings sc ON c.name_id = sc.id
         JOIN inheritance i ON c.id = i.child_id
         JOIN strings si ON i.parent_name_id = si.id
         JOIN files f ON c.file_id = f.id
         JOIN strings sp ON f.path_id = sp.id
         JOIN modules m ON f.module_id = m.id
         JOIN strings sm ON m.name_id = sm.id
         WHERE si.text = ?"
    )?;
    let rows = stmt.query_map([base_class], |row| {
        Ok(json!({
            "name": row.get::<_, String>(0)?,
            "base_class": row.get::<_, String>(1)?,
            "path": row.get::<_, String>(2)?,
            "module_name": row.get::<_, String>(3)?,
        }))
    })?;
    let res: Result<Vec<Value>, _> = rows.collect();
    Ok(json!(res?))
}

pub fn get_recursive_derived_classes(conn: &Connection, base_class: String) -> anyhow::Result<Value> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE derived_cte AS (
          SELECT c.id, sc.text as name, c.symbol_type FROM classes c JOIN strings sc ON c.name_id = sc.id WHERE sc.text = ?
          UNION ALL
          SELECT c.id, sc.text as name, c.symbol_type
          FROM classes c
          JOIN strings sc ON c.name_id = sc.id
          JOIN inheritance i ON c.id = i.child_id
          JOIN strings si ON i.parent_name_id = si.id
          JOIN derived_cte p ON si.text = p.name
        )
        SELECT d.name, '', c.line_number, sp.text as path, sfn.text as filename, d.symbol_type, sm.text as module_name
        FROM derived_cte d
        JOIN classes c ON d.id = c.id
        JOIN files f ON c.file_id = f.id
        JOIN strings sp ON f.path_id = sp.id
        JOIN strings sfn ON f.filename_id = sfn.id
        JOIN modules m ON f.module_id = m.id
        JOIN strings sm ON m.name_id = sm.id
        WHERE d.name != ?
        GROUP BY d.name"
    )?;
    let rows = stmt.query_map([&base_class, &base_class], |row| {
          Ok(json!({
              "class_name": row.get::<_, String>(0)?,
              "base_class": "",
              "line_number": row.get::<_, i64>(2)?,
              "file_path": row.get::<_, String>(3)?,
              "filename": row.get::<_, String>(4)?,
              "symbol_type": row.get::<_, String>(5)?,
              "module_name": row.get::<_, String>(6)?,
          }))
    })?;
    let res: Result<Vec<Value>, _> = rows.collect();
    Ok(json!(res?))
}

pub fn get_recursive_parent_classes(conn: &Connection, child_class: String) -> anyhow::Result<Value> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE parents_cte AS (
          SELECT c.id, 0 as level FROM classes c JOIN strings sc ON c.name_id = sc.id WHERE sc.text = ?
          UNION ALL
          SELECT i.parent_class_id, pc.level + 1
          FROM inheritance i
          JOIN parents_cte pc ON i.child_id = pc.id
          WHERE i.parent_class_id IS NOT NULL AND pc.level < 20
        )
        SELECT sc.text as name, '', c.line_number, sp.text as path, sfn.text as filename, c.symbol_type, sm.text as module_name, MIN(d.level) as min_level
        FROM parents_cte d
        JOIN classes c ON d.id = c.id
        JOIN strings sc ON c.name_id = sc.id
        JOIN files f ON c.file_id = f.id
        JOIN strings sp ON f.path_id = sp.id
        JOIN strings sfn ON f.filename_id = sfn.id
        LEFT JOIN modules m ON f.module_id = m.id
        LEFT JOIN strings sm ON m.name_id = sm.id
        GROUP BY sc.text
        ORDER BY min_level ASC"
    )?;
    let rows = stmt.query_map([child_class], |row| {
          Ok(json!({
              "class_name": row.get::<_, String>(0)?,
              "base_class": "",
              "line_number": row.get::<_, i64>(2)?,
              "file_path": row.get::<_, String>(3)?,
              "filename": row.get::<_, String>(4)?,
              "symbol_type": row.get::<_, String>(5)?,
              "module_name": row.get::<_, String>(6)?,
              "level": row.get::<_, i64>(7)?,
          }))
    })?;
    let res: Result<Vec<Value>, _> = rows.collect();
    Ok(json!(res?))
}

pub fn get_classes_in_modules(conn: &Connection, modules: Vec<String>, symbol_type: Option<String>) -> anyhow::Result<Value> {
    if modules.is_empty() { return Ok(json!([])); }
    let mut all_results = Vec::new();
    let mut groups: HashMap<String, Vec<Value>> = HashMap::new();
    let mut path_order: Vec<String> = Vec::new();

    for chunk in modules.chunks(500) {
        let placeholders: Vec<String> = chunk.iter().map(|_| "?".to_string()).collect();
        let mut sql = format!(
            "SELECT sc.text, sb.text, c.line_number, sp.text, c.symbol_type 
             FROM classes c 
             JOIN strings sc ON c.name_id = sc.id
             LEFT JOIN strings sb ON c.base_class_id = sb.id
             JOIN files f ON c.file_id = f.id 
             JOIN strings sp ON f.path_id = sp.id
             JOIN modules m ON f.module_id = m.id 
             JOIN strings sm ON m.name_id = sm.id
             WHERE sm.text IN ({})", 
            placeholders.join(",")
        );
        if let Some(st) = &symbol_type {
            match st.as_str() {
                "class" => sql.push_str(" AND (c.symbol_type = 'class' OR c.symbol_type = 'UCLASS' OR c.symbol_type = 'UINTERFACE')"),
                "struct" => sql.push_str(" AND (c.symbol_type = 'struct' OR c.symbol_type = 'USTRUCT')"),
                "enum" => sql.push_str(" AND (c.symbol_type = 'enum' OR c.symbol_type = 'UENUM')"),
                _ => sql.push_str(&format!(" AND c.symbol_type = '{}'", st)),
            }
        }
        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn ToSql> = chunk.iter().map(|s| s as &dyn ToSql).collect();
        let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, i64>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?))
        })?;
        for r in rows {
            let (name, base, line, path, stype) = r?;
            if symbol_type.is_some() {
                let item = json!([name, line, stype, base.unwrap_or_default()]);
                if !groups.contains_key(&path) { path_order.push(path.clone()); }
                groups.entry(path).or_default().push(item);
            } else { all_results.push(json!([name, line, path, stype, base])); }
        }
    }
    if symbol_type.is_some() {
        let res: Vec<Value> = path_order.into_iter().map(|path| { let items = groups.remove(&path).unwrap(); json!({ "p": path, "i": items }) }).collect();
        Ok(json!(res))
    } else { Ok(json!(all_results)) }
}

pub fn find_symbol_in_inheritance_chain(conn: &Connection, class_name: String, symbol_name: String, _mode: Option<String>) -> anyhow::Result<Value> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE parents_cte AS (
          SELECT c.id, sc.text as name, 0 as level 
          FROM classes c JOIN strings sc ON c.name_id = sc.id WHERE sc.text = ? 
          UNION 
          SELECT p.id, spc.text as name, pc.level + 1 
          FROM classes p 
          JOIN strings spc ON p.name_id = spc.id
          JOIN inheritance i ON p.id = i.child_id 
          JOIN strings si ON i.parent_name_id = si.id
          JOIN parents_cte pc ON si.text = pc.name
        ) 
        SELECT sp.text as path, m.line_number, p.name as class_name 
        FROM parents_cte p 
        JOIN members m ON p.id = m.class_id 
        JOIN strings sm ON m.name_id = sm.id
        JOIN classes c ON p.id = c.id 
        JOIN files f ON c.file_id = f.id 
        JOIN strings sp ON f.path_id = sp.id
        WHERE sm.text = ? AND p.level >= 0 
        ORDER BY p.level ASC LIMIT 1"
    )?;
    let res = stmt.query_row(params![class_name, symbol_name], |row| Ok(json!({ "file_path": row.get::<_, String>(0)?, "line_number": row.get::<_, i64>(1)?, "class_name": row.get::<_, String>(2)? }))).optional()?;
    
    // If found, check if there's a corresponding implementation in .cpp
    if res.is_some() {
        let _data = res.as_ref().unwrap();
        
        // Try to find if this member has an entry from a .cpp file (impl access or same name in .cpp)
        let mut stmt_impl = conn.prepare(
            "SELECT sp.text, m.line_number 
             FROM members m
             JOIN strings sm ON m.name_id = sm.id
             JOIN classes c ON m.class_id = c.id
             JOIN strings sc ON c.name_id = sc.id
             JOIN files f ON m.file_id = f.id
             JOIN strings sp ON f.path_id = sp.id
             WHERE sc.text = ? AND sm.text = ? AND f.extension IN ('cpp', 'c', 'cc')
             LIMIT 1"
        )?;
        
        let res_impl = stmt_impl.query_row(params![class_name, symbol_name], |row| Ok(json!({ 
            "file_path": row.get::<_, String>(0)?, 
            "line_number": row.get::<_, i64>(1)?,
            "class_name": class_name.clone()
        }))).optional()?;
        
        if res_impl.is_some() { return Ok(json!(res_impl)); }
    }
    
    Ok(json!(res))
}

pub fn get_virtual_functions_in_inheritance_chain(conn: &Connection, class_name: String) -> anyhow::Result<Value> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE parents_cte AS (
          SELECT c.id, sc.text as name, 0 as level 
          FROM classes c JOIN strings sc ON c.name_id = sc.id WHERE sc.text = ? 
          UNION 
          SELECT p.id, spc.text as name, pc.level + 1 
          FROM classes p 
          JOIN strings spc ON p.name_id = spc.id
          JOIN inheritance i ON p.id = i.child_id 
          JOIN strings si ON i.parent_name_id = si.id
          JOIN parents_cte pc ON si.text = pc.name
        ) 
        SELECT smn.text as name, smt.text as type, m.flags, srt.text as return_type, m.detail, m.line_number, sp.text as path, p.name as class_name 
        FROM parents_cte p 
        JOIN members m ON p.id = m.class_id 
        JOIN strings smn ON m.name_id = smn.id
        JOIN strings smt ON m.type_id = smt.id
        LEFT JOIN strings srt ON m.return_type_id = srt.id
        JOIN classes c ON p.id = c.id 
        JOIN files f ON c.file_id = f.id 
        JOIN strings sp ON f.path_id = sp.id
        WHERE m.flags LIKE '%virtual%' 
        ORDER BY p.level ASC, smn.text ASC"
    )?;
    let rows = stmt.query_map([class_name], |row| Ok(json!({ "name": row.get::<_, String>(0)?, "kind": row.get::<_, String>(1)?, "flags": row.get::<_, Option<String>>(2)?, "return_type": row.get::<_, Option<String>>(3)?, "params": row.get::<_, Option<String>>(4)?, "line": row.get::<_, i64>(5)?, "file_path": row.get::<_, String>(6)?, "declared_in": row.get::<_, String>(7)?, "is_virtual": true })))?;
    let res: Result<Vec<Value>, _> = rows.collect();
    Ok(json!(res?))
}

pub fn find_class_by_name(conn: &Connection, name: String) -> anyhow::Result<Value> {
    let mut stmt = conn.prepare(
        "SELECT c.id, sc.text as name, sb.text as base_class, c.line_number, sp.text as path, sfn.text as filename, c.symbol_type, sm.text as module_name, sr.text as module_root 
         FROM classes c 
         JOIN strings sc ON c.name_id = sc.id
         LEFT JOIN strings sb ON c.base_class_id = sb.id
         JOIN files f ON c.file_id = f.id 
         JOIN strings sp ON f.path_id = sp.id
         JOIN strings sfn ON f.filename_id = sfn.id
         JOIN modules m ON f.module_id = m.id 
         JOIN strings sm ON m.name_id = sm.id
         JOIN strings sr ON m.root_path_id = sr.id
         WHERE sc.text = ? LIMIT 1"
    )?;
    let res = stmt.query_row([name], |row| Ok(json!({ "id": row.get::<_, i64>(0)?, "class_name": row.get::<_, String>(1)?, "base_class": row.get::<_, Option<String>>(2)?, "line_number": row.get::<_, i64>(3)?, "file_path": row.get::<_, String>(4)?, "filename": row.get::<_, String>(5)?, "symbol_type": row.get::<_, String>(6)?, "module_name": row.get::<_, String>(7)?, "module_root": row.get::<_, String>(8)? }))).optional()?;
    Ok(json!(res))
}

pub fn search_classes_prefix(conn: &Connection, prefix: String, limit: Option<usize>) -> anyhow::Result<Value> {
    let mut stmt = conn.prepare("SELECT s.text, symbol_type FROM classes c JOIN strings s ON c.name_id = s.id WHERE s.text LIKE ? LIMIT ?")?;
    let param = format!("{}%", prefix);
    let lim = limit.unwrap_or(50) as i64;
    let rows = stmt.query_map(params![param, lim], |row| Ok(json!({ "name": row.get::<_, String>(0)?, "symbol_type": row.get::<_, String>(1)? })))?;
    Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
}

pub fn search_symbols_in_modules(conn: &Connection, _modules: Vec<String>, _symbol_type: Option<String>, filter: String, limit: Option<usize>) -> anyhow::Result<Value> {
     let limit_val = limit.unwrap_or(100);
     
     // 1. Try FTS5 for lightning fast fuzzy matching
     let mut stmt_fts = conn.prepare(
        "SELECT f.name, f.type, f.class_name, f.rowid_ref, c.line_number, sp.text, sm.text
         FROM symbols_fts f
         JOIN classes c ON f.rowid_ref = c.id
         JOIN files fi ON c.file_id = fi.id
         JOIN strings sp ON fi.path_id = sp.id
         JOIN modules m ON fi.module_id = m.id
         JOIN strings sm ON m.name_id = sm.id
         WHERE f.name MATCH ? LIMIT ?"
     )?;
     
     let fts_query = format!("{}*", filter.replace("\"", ""));
     let fts_rows = stmt_fts.query_map(params![fts_query, limit_val as i64], |row| {
         Ok(json!({
             "name": row.get::<_, String>(0)?,
             "symbol_type": row.get::<_, String>(1)?,
             "class_name": row.get::<_, String>(2)?,
             "line_number": row.get::<_, i64>(4)?,
             "path": row.get::<_, String>(5)?,
             "module_name": row.get::<_, String>(6)?,
         }))
     })?;

     let results: Vec<Value> = fts_rows.filter_map(|r| r.ok()).collect();
     if !results.is_empty() {
         return Ok(json!(results));
     }

     // 2. Fallback to classic module-restricted search (simplified)
     Ok(json!([]))
}

pub fn get_class_members_recursive(conn: &Connection, class_name: String, namespace: Option<String>) -> anyhow::Result<Value> {
    let mut result_members = Vec::new();
    let mut visited = HashMap::new();
    let mut queue = vec![class_name.clone()];
    while let Some(current_name) = queue.pop() {
        let (search_name, _search_ns) = if let Some(idx) = current_name.find("::") { (current_name[idx+2..].to_string(), Some(current_name[0..idx].to_string())) } else { (current_name.clone(), namespace.clone()) };
        let visited_key = format!("{}::{}", _search_ns.unwrap_or_default(), search_name);
        if visited.contains_key(&visited_key) { continue; }
        visited.insert(visited_key, true);
        let mut stmt = conn.prepare("SELECT c.id FROM classes c JOIN strings s ON c.name_id = s.id WHERE s.text = ? LIMIT 1")?;
        if let Some(class_id) = stmt.query_row(params![search_name], |row| Ok(row.get::<_, i64>(0)?)).optional()? {
            let mut mem_stmt = conn.prepare(
                "SELECT sn.text, st.text, m.flags, m.access, m.detail, srt.text, m.is_static, m.line_number 
                 FROM members m 
                 JOIN strings sn ON m.name_id = sn.id
                 JOIN strings st ON m.type_id = st.id
                 LEFT JOIN strings srt ON m.return_type_id = srt.id
                 WHERE m.class_id = ? ORDER BY st.text, sn.text"
            )?;
            let mem_iter = mem_stmt.query_map([class_id], |row| Ok(json!({ "name": row.get::<_, String>(0)?, "type": row.get::<_, String>(1)?, "flags": row.get::<_, Option<String>>(2)?, "access": row.get::<_, Option<String>>(3)?, "detail": row.get::<_, Option<String>>(4)?, "return_type": row.get::<_, Option<String>>(5)?, "is_static": row.get::<_, i64>(6)? == 1, "class_name": search_name.clone() })))?;
            for m in mem_iter { let m = m?; let name = m["name"].as_str().unwrap(); if !result_members.iter().any(|existing: &Value| existing["name"].as_str() == Some(name)) { result_members.push(m); } }
            let mut p_stmt = conn.prepare(
                "SELECT si.text FROM inheritance i 
                 JOIN strings si ON i.parent_name_id = si.id
                 WHERE i.child_id = ? AND si.text != ?"
            )?;
            let p_rows = p_stmt.query_map(params![class_id, search_name], |row| Ok(row.get::<_, String>(0)?))?;
            for p in p_rows { queue.push(p?); }
        }
    }
    Ok(json!(result_members))
}

pub fn get_file_symbols(conn: &Connection, file_path: String) -> anyhow::Result<Value> {
    let normalized_path = if std::path::MAIN_SEPARATOR == '\\' { file_path.replace('\\', "/") } else { file_path.clone() };
    
    // 1. Get current file info
    let file_info = conn.query_row(
        "SELECT f.filename_id, f.module_id FROM files f 
         JOIN strings s ON f.path_id = s.id 
         WHERE LOWER(s.text) = LOWER(?)",
        [&normalized_path],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
    ).optional()?;

    let (filename_id, module_id) = match file_info {
        Some(info) => info,
        None => return Ok(json!([])),
    };

    // 2. Get the stem (filename without extension)
    let filename: String = conn.query_row("SELECT text FROM strings WHERE id = ?", [filename_id], |r| r.get(0))?;
    let stem = filename.split('.').next().unwrap_or(&filename);

    // 3. Find all files in the same module with the same stem (e.g. MyActor.h, MyActor.cpp)
    let mut stmt = conn.prepare(
        "SELECT f.id, sp.text as full_path, sm.text as module_name, sr.text as module_root 
         FROM files f 
         JOIN strings sp ON f.path_id = sp.id
         JOIN strings sn ON f.filename_id = sn.id
         JOIN modules m ON f.module_id = m.id
         JOIN strings sm ON m.name_id = sm.id
         JOIN strings sr ON m.root_path_id = sr.id
         WHERE f.module_id = ? AND (sn.text = ? OR sn.text LIKE ?)"
    )?;
    let target_like = format!("{}.%", stem);
    let related_files: Vec<(i64, String, String, String)> = stmt.query_map(params![module_id, stem, target_like], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    })?.filter_map(|r| r.ok()).collect();

    if related_files.is_empty() { return Ok(json!([])); }

    let file_ids: Vec<i64> = related_files.iter().map(|f| f.0).collect();
    let placeholders: Vec<String> = file_ids.iter().map(|_| "?".to_string()).collect();
    let sql_ids = placeholders.join(",");

    // 4. Find all class_ids associated with these files (Classes only)
    let mut stmt = conn.prepare(&format!(
        "SELECT id FROM classes WHERE file_id IN ({})", sql_ids
    ))?;
    let mut params_ids: Vec<&dyn ToSql> = Vec::new();
    for id in &file_ids { params_ids.push(id); }
    
    let class_ids: Vec<i64> = stmt.query_map(rusqlite::params_from_iter(params_ids), |row| row.get(0))?
        .filter_map(|r| r.ok()).collect();

    let mut results = Vec::new();

    for cid in class_ids {
        // 5. Fetch full class info
        let class_info = conn.query_row(
            "SELECT sc.text as name, c.symbol_type, c.line_number, sns.text as namespace, sb.text as base_class, c.end_line_number, sm.text as module_name, sr.text as module_root, sp.text as file_path
             FROM classes c 
             JOIN strings sc ON c.name_id = sc.id
             LEFT JOIN strings sns ON c.namespace_id = sns.id
             LEFT JOIN strings sb ON c.base_class_id = sb.id
             JOIN files f ON c.file_id = f.id
             JOIN strings sp ON f.path_id = sp.id
             LEFT JOIN modules m ON f.module_id = m.id
             LEFT JOIN strings sm ON m.name_id = sm.id
             LEFT JOIN strings sr ON m.root_path_id = sr.id
             WHERE c.id = ?",
            [cid],
            |row| Ok(json!({
                "name": row.get::<_, String>(0)?,
                "kind": row.get::<_, String>(1)?,
                "line": row.get::<_, i64>(2)?,
                "namespace": row.get::<_, Option<String>>(3)?,
                "base_class": row.get::<_, Option<String>>(4)?,
                "end_line": row.get::<_, Option<i64>>(5)?,
                "module_name": row.get::<_, Option<String>>(6)?,
                "module_root": row.get::<_, Option<String>>(7)?,
                "file_path": row.get::<_, String>(8)?,
                "fields": { "public": [], "protected": [], "private": [] },
                "methods": { "public": [], "protected": [], "private": [] }
            }))
        ).optional()?;

        let mut class_json = match class_info {
            Some(info) => info,
            None => continue,
        };

        let current_class_name = class_json["name"].as_str().unwrap_or("").to_string();

        // 6. Fetch ALL members for this class (from ALL files related to this class)
        let mut mem_stmt = conn.prepare(
            "SELECT sn.text as name, st.text as type, m.flags, m.access, m.detail, srt.text as return_type, m.is_static, m.line_number, sp.text as file_path
             FROM members m
             JOIN strings sn ON m.name_id = sn.id
             JOIN strings st ON m.type_id = st.id
             LEFT JOIN strings srt ON m.return_type_id = srt.id
             JOIN files f ON m.file_id = f.id
             JOIN strings sp ON f.path_id = sp.id
             WHERE m.class_id = ? ORDER BY m.line_number"
        )?;

        let mem_rows = mem_stmt.query_map([cid], |row| {
            let m_name: String = row.get(0)?;
            let m_type: String = row.get(1)?;
            let m_path: String = row.get(8)?;
            
            let mut access = row.get::<_, Option<String>>(3)
                .unwrap_or(Some("public".to_string()))
                .unwrap_or("public".to_string())
                .to_lowercase();
            
            // sourceファイル (.cpp, .c, .cc) からのメンバは 'impl' として扱う (関数の場合)
            if m_type.to_lowercase().contains("function") {
                let p_lower = m_path.to_lowercase();
                if p_lower.ends_with(".cpp") || p_lower.ends_with(".c") || p_lower.ends_with(".cc") {
                    access = "impl".to_string();
                }
            }

            // Picker表示用にクラス名をカッコで付与するヒントを追加
            let display_name = format!("{} ({})", m_name, current_class_name);

            Ok((access, m_type, json!({
                "name": m_name,
                "display": display_name,
                "kind": row.get::<_, String>(1)?,
                "flags": row.get::<_, Option<String>>(2)?,
                "access": row.get::<_, Option<String>>(3)?,
                "detail": row.get::<_, Option<String>>(4)?,
                "return_type": row.get::<_, Option<String>>(5)?,
                "is_static": row.get::<_, i64>(6)? == 1,
                "line": row.get::<_, i64>(7)?,
                "file_path": m_path
            })))
        })?;

        for m_res in mem_rows {
            let (access, m_type, m_json) = m_res?;
            let target = if m_type.to_lowercase().contains("function") { "methods" } else { "fields" };
            if let Some(map) = class_json[target].as_object_mut() {
                map.entry(access).or_insert(json!([])).as_array_mut().unwrap().push(m_json);
            }
        }

        // 7. If it's an Enum, fetch its values from enum_values table
        let k_lower = class_json["kind"].as_str().unwrap_or("").to_lowercase();
        if k_lower == "uenum" || k_lower == "enum" {
            let mut enum_stmt = conn.prepare(
                "SELECT sen.text, ev.line_number, sp.text as file_path
                 FROM enum_values ev
                 JOIN strings sen ON ev.name_id = sen.id
                 JOIN files f ON ev.file_id = f.id
                 JOIN strings sp ON f.path_id = sp.id
                 WHERE ev.enum_id = ? ORDER BY ev.line_number"
            )?;
            let enum_rows = enum_stmt.query_map([cid], |row| {
                let name: String = row.get(0)?;
                Ok(json!({
                    "name": name,
                    "kind": "Field",
                    "access": "public",
                    "line": row.get::<_, i64>(1)?,
                    "file_path": row.get::<_, String>(2)?
                }))
            })?;
            for e_res in enum_rows {
                if let Ok(e_json) = e_res {
                    if let Some(fields) = class_json["fields"].as_object_mut() {
                        fields.entry("public".to_string()).or_insert(json!([])).as_array_mut().unwrap().push(e_json);
                    }
                }
            }
        }

        results.push(class_json);
    }

    Ok(json!(results))
}

pub fn find_symbol_usages(conn: &Connection, symbol_name: String, limit: Option<usize>) -> anyhow::Result<Value> {
    let mut stmt = conn.prepare(
        "SELECT sp.text as path, c.line
         FROM symbol_calls c
         JOIN strings ss ON c.name_id = ss.id
         JOIN files f ON c.file_id = f.id
         JOIN strings sp ON f.path_id = sp.id
         WHERE ss.text = ?
         LIMIT ?"
    )?;
    
    let lim = limit.unwrap_or(500) as i64;
    let rows = stmt.query_map(params![symbol_name, lim], |row| {
        Ok(json!({
            "path": row.get::<_, String>(0)?,
            "line": row.get::<_, i64>(1)?,
        }))
    })?;
    
    let res: Result<Vec<Value>, _> = rows.collect();
    Ok(json!(res?))
}