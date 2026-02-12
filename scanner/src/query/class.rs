use rusqlite::{params, Connection, ToSql, OptionalExtension};
use serde_json::{json, Value};
use std::collections::HashMap;

pub fn find_derived_classes(conn: &Connection, base_class: String) -> anyhow::Result<Value> {
    let mut stmt = conn.prepare(
        "SELECT c.name, '' as base_class, f.path, m.name as module_name
         FROM classes c
         JOIN inheritance i ON c.id = i.child_id
         JOIN files f ON c.file_id = f.id
         JOIN modules m ON f.module_id = m.id
         WHERE i.parent_name = ?"
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
          SELECT id, name, symbol_type FROM classes WHERE name = ?
          UNION ALL
          SELECT c.id, c.name, c.symbol_type
          FROM classes c
          JOIN inheritance i ON c.id = i.child_id
          JOIN derived_cte p ON i.parent_name = p.name
        )
        SELECT d.name, '', c.line_number, f.path, f.filename, d.symbol_type, m.name
        FROM derived_cte d
        JOIN classes c ON d.id = c.id
        JOIN files f ON c.file_id = f.id
        JOIN modules m ON f.module_id = m.id
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
          SELECT id, name, 0 as level FROM classes WHERE name = ?
          UNION ALL
          SELECT p.id, p.name, c.level + 1
          FROM classes p
          JOIN inheritance i ON p.name = i.parent_name
          JOIN parents_cte c ON i.child_id = c.id
        )
        SELECT d.name, '', c.line_number, f.path, f.filename, c.symbol_type, m.name, MIN(d.level) as min_level
        FROM parents_cte d
        JOIN classes c ON d.id = c.id
        JOIN files f ON c.file_id = f.id
        JOIN modules m ON f.module_id = m.id
        GROUP BY d.name
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
        let mut sql = format!("SELECT c.name, c.base_class, c.line_number, f.path, c.symbol_type FROM classes c JOIN files f ON c.file_id = f.id JOIN modules m ON f.module_id = m.id WHERE m.name IN ({})", placeholders.join(","));
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

pub fn find_symbol_in_inheritance_chain(conn: &Connection, class_name: String, symbol_name: String, mode: Option<String>) -> anyhow::Result<Value> {
    let is_impl = mode.unwrap_or_default() == "implementation";
    let mut stmt = conn.prepare("WITH RECURSIVE parents_cte AS (SELECT id, name, 0 as level FROM classes WHERE name = ? UNION SELECT p.id, p.name, pc.level + 1 FROM classes p JOIN inheritance i ON p.name = i.parent_name JOIN parents_cte pc ON i.child_id = pc.id) SELECT f.path, m.line_number, p.name as class_name FROM parents_cte p JOIN members m ON p.id = m.class_id JOIN classes c ON p.id = c.id JOIN files f ON c.file_id = f.id WHERE m.name = ? AND p.level > 0 ORDER BY p.level ASC LIMIT 1")?;
    let res = stmt.query_row(params![class_name, symbol_name], |row| Ok(json!({ "file_path": row.get::<_, String>(0)?, "line_number": row.get::<_, i64>(1)?, "class_name": row.get::<_, String>(2)? }))).optional()?;
    if is_impl && res.is_some() {
        let data = res.as_ref().unwrap();
        let h_path = data["file_path"].as_str().unwrap();
        let c_name = data["class_name"].as_str().unwrap();
        let h_stem = std::path::Path::new(h_path).file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let mut stmt_cpp = conn.prepare("SELECT f.path FROM files f WHERE f.module_id = (SELECT module_id FROM files WHERE path = ?) AND f.extension IN ('cpp', 'c', 'cc') AND f.filename LIKE ? LIMIT 1")?;
        let target_like = format!("{}%.cpp", h_stem);
        let res_cpp = stmt_cpp.query_row(params![h_path, target_like], |row| Ok(json!({ "file_path": row.get::<_, String>(0)?, "line_number": 0, "class_name": c_name }))).optional()?;
        if res_cpp.is_some() { return Ok(json!(res_cpp)); }
    }
    Ok(json!(res))
}

pub fn get_virtual_functions_in_inheritance_chain(conn: &Connection, class_name: String) -> anyhow::Result<Value> {
    let mut stmt = conn.prepare("WITH RECURSIVE parents_cte AS (SELECT id, name, 0 as level FROM classes WHERE name = ? UNION SELECT p.id, p.name, pc.level + 1 FROM classes p JOIN inheritance i ON p.name = i.parent_name JOIN parents_cte pc ON i.child_id = pc.id) SELECT m.name, m.type, m.flags, m.return_type, m.detail, m.line_number, f.path, p.name as class_name FROM parents_cte p JOIN members m ON p.id = m.class_id JOIN classes c ON p.id = c.id JOIN files f ON c.file_id = f.id WHERE m.flags LIKE '%virtual%' ORDER BY p.level ASC, m.name ASC")?;
    let rows = stmt.query_map([class_name], |row| Ok(json!({ "name": row.get::<_, String>(0)?, "kind": row.get::<_, String>(1)?, "flags": row.get::<_, Option<String>>(2)?, "return_type": row.get::<_, Option<String>>(3)?, "params": row.get::<_, Option<String>>(4)?, "line": row.get::<_, i64>(5)?, "file_path": row.get::<_, String>(6)?, "declared_in": row.get::<_, String>(7)?, "is_virtual": true })))?;
    let res: Result<Vec<Value>, _> = rows.collect();
    Ok(json!(res?))
}

pub fn find_class_by_name(conn: &Connection, name: String) -> anyhow::Result<Value> {
    let mut stmt = conn.prepare("SELECT c.id, c.name, c.base_class, c.line_number, f.path, f.filename, c.symbol_type, m.name, m.root_path FROM classes c JOIN files f ON c.file_id = f.id JOIN modules m ON f.module_id = m.id WHERE c.name = ? LIMIT 1")?;
    let res = stmt.query_row([name], |row| Ok(json!({ "id": row.get::<_, i64>(0)?, "class_name": row.get::<_, String>(1)?, "base_class": row.get::<_, Option<String>>(2)?, "line_number": row.get::<_, i64>(3)?, "file_path": row.get::<_, String>(4)?, "filename": row.get::<_, String>(5)?, "symbol_type": row.get::<_, String>(6)?, "module_name": row.get::<_, String>(7)?, "module_root": row.get::<_, String>(8)? }))).optional()?;
    Ok(json!(res))
}

pub fn search_classes_prefix(conn: &Connection, prefix: String, limit: Option<usize>) -> anyhow::Result<Value> {
    let mut stmt = conn.prepare("SELECT name, symbol_type FROM classes WHERE name LIKE ? LIMIT ?")?;
    let param = format!("{}%", prefix);
    let lim = limit.unwrap_or(50) as i64;
    let rows = stmt.query_map(params![param, lim], |row| Ok(json!({ "name": row.get::<_, String>(0)?, "symbol_type": row.get::<_, String>(1)? })))?;
    Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
}

pub fn search_symbols_in_modules(conn: &Connection, modules: Vec<String>, symbol_type: Option<String>, filter: String, limit: Option<usize>) -> anyhow::Result<Value> {
     if modules.is_empty() { return Ok(json!([])); }
     let limit_val = limit.unwrap_or(100);
     let mut all_results = Vec::new();
     for chunk in modules.chunks(500) {
         if all_results.len() >= limit_val { break; }
         let remaining = limit_val - all_results.len();
         let placeholders: Vec<String> = chunk.iter().map(|_| "?".to_string()).collect();
         let mut sql = format!("SELECT c.name, c.base_class, c.line_number, f.path, c.symbol_type, m.name FROM classes c JOIN files f ON c.file_id = f.id JOIN modules m ON f.module_id = m.id WHERE m.name IN ({}) AND c.name LIKE ?", placeholders.join(","));
         if let Some(st) = &symbol_type { match st.as_str() { "class" => sql.push_str(" AND (c.symbol_type = 'class' OR c.symbol_type = 'UCLASS' OR c.symbol_type = 'UINTERFACE')"), "struct" => sql.push_str(" AND (c.symbol_type = 'struct' OR c.symbol_type = 'USTRUCT')"), "enum" => sql.push_str(" AND (c.symbol_type = 'enum' OR c.symbol_type = 'UENUM')"), _ => sql.push_str(&format!(" AND c.symbol_type = '{}'", st)), } }
         sql.push_str(" LIMIT ?");
         let filter_param = format!("%{}%", filter);
         let mut params: Vec<&dyn ToSql> = chunk.iter().map(|s| s as &dyn ToSql).collect();
         params.push(&filter_param);
         let limit_param = remaining as i64;
         params.push(&limit_param);
         let mut stmt = conn.prepare(&sql)?;
         let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| Ok(json!({ "name": row.get::<_, String>(0)?, "base_class": row.get::<_, Option<String>>(1)?, "line_number": row.get::<_, i64>(2)?, "path": row.get::<_, String>(3)?, "symbol_type": row.get::<_, String>(4)?, "module_name": row.get::<_, String>(5)? })))?;
         for r in rows { all_results.push(r?); }
     }
     Ok(json!(all_results))
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
        let mut stmt = conn.prepare("SELECT c.id FROM classes c JOIN files f ON c.file_id = f.id WHERE c.name = ? LIMIT 1")?;
        if let Some(class_id) = stmt.query_row(params![search_name], |row| Ok(row.get::<_, i64>(0)?)).optional()? {
            let mut mem_stmt = conn.prepare("SELECT name, type, flags, access, detail, return_type, is_static, line_number FROM members WHERE class_id = ? ORDER BY type, name")?;
            let mem_iter = mem_stmt.query_map([class_id], |row| Ok(json!({ "name": row.get::<_, String>(0)?, "type": row.get::<_, String>(1)?, "flags": row.get::<_, Option<String>>(2)?, "access": row.get::<_, Option<String>>(3)?, "detail": row.get::<_, Option<String>>(4)?, "return_type": row.get::<_, Option<String>>(5)?, "is_static": row.get::<_, i64>(6)? == 1, "class_name": search_name.clone() })))?;
            for m in mem_iter { let m = m?; let name = m["name"].as_str().unwrap(); if !result_members.iter().any(|existing: &Value| existing["name"].as_str() == Some(name)) { result_members.push(m); } }
            let mut p_stmt = conn.prepare("SELECT parent_name FROM inheritance WHERE child_id = ? AND parent_name != ?")?;
            let p_rows = p_stmt.query_map(params![class_id, search_name], |row| Ok(row.get::<_, String>(0)?))?;
            for p in p_rows { queue.push(p?); }
        }
    }
    Ok(json!(result_members))
}

pub fn get_file_symbols(conn: &Connection, file_path: String) -> anyhow::Result<Value> {
    let normalized_path = if std::path::MAIN_SEPARATOR == '\\' { file_path.replace('\\', "/") } else { file_path.clone() };
    let mut stmt = conn.prepare("SELECT c.id, c.name, c.symbol_type, c.line_number, c.namespace, c.base_class, c.end_line_number, m.name, m.root_path FROM classes c JOIN files f ON c.file_id = f.id LEFT JOIN modules m ON f.module_id = m.id WHERE LOWER(f.path) = LOWER(?)")?;
    let class_rows = stmt.query_map([&normalized_path], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?, row.get::<_, Option<String>>(4)?, row.get::<_, Option<String>>(5)?, row.get::<_, Option<i64>>(6)?, row.get::<_, Option<String>>(7)?, row.get::<_, Option<String>>(8)?)))?;
    let mut results = Vec::new();
    for r in class_rows {
        let (cid, cname, ctype, cline, cns, cbase, cend, mname, mroot) = r?;
        let mut class_info = json!({ "name": cname, "kind": ctype, "line": cline, "end_line": cend.unwrap_or(cline), "namespace": cns, "base_class": cbase, "file_path": file_path, "module_name": mname, "module_root": mroot, "fields": { "public": [], "protected": [], "private": [] }, "methods": { "public": [], "protected": [], "private": [] } });
        let mut mem_stmt = conn.prepare("SELECT name, type, flags, access, detail, return_type, is_static, line_number FROM members WHERE class_id = ? ORDER BY name")?;
        let mem_rows = mem_stmt.query_map([cid], |row| Ok(json!({ "name": row.get::<_, String>(0)?, "kind": row.get::<_, String>(1)?, "flags": row.get::<_, Option<String>>(2)?, "access": row.get::<_, Option<String>>(3)?, "detail": row.get::<_, Option<String>>(4)?, "return_type": row.get::<_, Option<String>>(5)?, "is_static": row.get::<_, i64>(6)? == 1, "file_path": file_path, "line": row.get::<_, i64>(7)? })))?;
        for m_res in mem_rows {
            let m = m_res?;
            let access = m["access"].as_str().unwrap_or("public").to_lowercase();
            let target = if m["kind"].as_str().unwrap_or("").to_lowercase().contains("function") { "methods" } else { "fields" };
            class_info[target].as_object_mut().unwrap().entry(access).or_insert(json!([])).as_array_mut().unwrap().push(m);
        }
        results.push(class_info);
    }
    Ok(json!(results))
}