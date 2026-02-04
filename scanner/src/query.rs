use rusqlite::{params, Connection, OptionalExtension, ToSql};
use crate::types::QueryRequest;
use serde_json::{json, Value};
use std::collections::HashMap;

pub fn process_query(db_path: &str, req: QueryRequest) -> anyhow::Result<Value> {
    let conn = Connection::open(db_path)?;
    
    match req {
        QueryRequest::FindDerivedClasses { base_class } => {
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
        },
        QueryRequest::SearchFiles { part } => {
             let mut stmt = conn.prepare(
                "SELECT path, filename FROM files WHERE filename LIKE ? LIMIT 100"
            )?;
            let param = format!("%{}%", part);
            let rows = stmt.query_map([param], |row| {
                Ok(json!({
                    "path": row.get::<_, String>(0)?,
                    "filename": row.get::<_, String>(1)?,
                }))
            })?;
            let res: Result<Vec<Value>, _> = rows.collect();
            Ok(json!(res?))
        },
        QueryRequest::LoadComponentData { component } => {
             let mut stmt = conn.prepare(
                "SELECT m.id, m.name, m.type, m.scope, m.root_path, m.build_cs_path
                 FROM modules m
                 WHERE m.scope = ? OR m.scope LIKE ?"
             )?;
             let param2 = format!("{}%", component);
             let modules_iter = stmt.query_map(params![component, param2], |row| {
                 Ok((
                     row.get::<_, i64>(0)?,
                     row.get::<_, String>(1)?,
                     row.get::<_, String>(2)?,
                     row.get::<_, String>(4)?,
                     row.get::<_, Option<String>>(5)?,
                 ))
             })?;

             let mut result = json!({
                 "runtime_modules": {},
                 "editor_modules": {},
                 "developer_modules": {},
                 "programs_modules": {}
             });

             let mut file_stmt = conn.prepare(
                 "SELECT f.id, f.path, f.filename, f.extension, f.is_header, f.module_id
                  FROM files f
                  WHERE f.module_id = ?"
             )?;
             
             let mut class_stmt = conn.prepare(
                 "SELECT c.name, c.base_class, c.line_number
                  FROM classes c
                  WHERE c.file_id = ?"
             )?;

             for mod_res in modules_iter {
                 let (mid, mname, mtype, mroot, mpath) = mod_res?;
                 let mut mod_data = json!({
                     "name": mname,
                     "module_root": mroot,
                     "path": mpath,
                     "files": { "source": [], "config": [], "shader": [], "other": [] },
                     "header_details": {}
                 });

                 let files_iter = file_stmt.query_map([mid], |row| {
                     Ok((
                         row.get::<_, i64>(0)?,
                         row.get::<_, String>(1)?,
                         row.get::<_, String>(3)?, // extension
                         row.get::<_, i64>(4)?, // is_header
                     ))
                 })?;

                 for file_res in files_iter {
                     let (fid, fpath, fext, is_header) = file_res?;
                     let ext = fext.to_lowercase();
                     
                     if ["cpp", "c", "cc", "h", "hpp"].contains(&ext.as_str()) {
                         mod_data["files"]["source"].as_array_mut().unwrap().push(json!(fpath));
                         if is_header == 1 {
                             let classes_iter = class_stmt.query_map([fid], |row| {
                                 Ok(json!({
                                     "name": row.get::<_, String>(0)?,
                                     "base_class": row.get::<_, Option<String>>(1)?,
                                     "line_number": row.get::<_, i64>(2)?,
                                 }))
                             })?;
                             let classes: Vec<Value> = classes_iter.collect::<Result<_, _>>()?;
                             if !classes.is_empty() {
                                 mod_data["header_details"].as_object_mut().unwrap().insert(fpath, json!({ "classes": classes }));
                             }
                         }
                     } else if ext == "ini" {
                         mod_data["files"]["config"].as_array_mut().unwrap().push(json!(fpath));
                     } else if ext == "usf" || ext == "ush" {
                         mod_data["files"]["shader"].as_array_mut().unwrap().push(json!(fpath));
                     } else {
                         mod_data["files"]["other"].as_array_mut().unwrap().push(json!(fpath));
                     }
                 }

                 let target_key = match mtype.as_str() {
                     "Runtime" => "runtime_modules",
                     "Editor" => "editor_modules",
                     "Developer" => "developer_modules",
                     "Program" => "programs_modules",
                     _ => "runtime_modules", // Default
                 };
                 result[target_key].as_object_mut().unwrap().insert(mname, mod_data);
             }
             Ok(result)
        },
        QueryRequest::GetModuleByName { name } => {
            let mut stmt = conn.prepare(
                "SELECT m.id, m.name, m.type, m.scope, m.root_path, m.build_cs_path
                 FROM modules m WHERE m.name = ? LIMIT 1"
            )?;
            let mod_row = stmt.query_row([name], |row| {
                Ok((
                     row.get::<_, i64>(0)?,
                     row.get::<_, String>(1)?,
                     row.get::<_, String>(4)?,
                     row.get::<_, Option<String>>(5)?,
                ))
            }).optional()?;

            if let Some((mid, mname, mroot, mpath)) = mod_row {
                let mut mod_data = json!({
                     "name": mname,
                     "module_root": mroot,
                     "path": mpath,
                     "files": { "source": [], "config": [], "shader": [], "other": [] }
                });
                
                let mut stmt = conn.prepare("SELECT f.path, f.extension FROM files f WHERE f.module_id = ?")?;
                let files_iter = stmt.query_map([mid], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?;

                for res in files_iter {
                    let (fpath, fext) = res?;
                    let ext = fext.to_lowercase();
                    if ["cpp", "c", "cc", "h", "hpp"].contains(&ext.as_str()) {
                        mod_data["files"]["source"].as_array_mut().unwrap().push(json!(fpath));
                    } else if ext == "ini" {
                         mod_data["files"]["config"].as_array_mut().unwrap().push(json!(fpath));
                    } else if ext == "usf" || ext == "ush" {
                         mod_data["files"]["shader"].as_array_mut().unwrap().push(json!(fpath));
                    } else {
                         mod_data["files"]["other"].as_array_mut().unwrap().push(json!(fpath));
                    }
                }
                Ok(mod_data)
            } else {
                Ok(Value::Null)
            }
        },
        QueryRequest::GetClassesInModules { modules, symbol_type } => {
             if modules.is_empty() { return Ok(json!([])); }
             
             let mut all_results = Vec::new();
             let mut groups: HashMap<String, Vec<Value>> = HashMap::new();
             let mut path_order: Vec<String> = Vec::new();

             for chunk in modules.chunks(500) {
                 let placeholders: Vec<String> = chunk.iter().map(|_| "?".to_string()).collect();
                 let mut sql = format!(
                     "SELECT c.name as class_name, c.base_class, c.line_number, f.path as file_path, c.symbol_type
                      FROM classes c
                      JOIN files f ON c.file_id = f.id
                      JOIN modules m ON f.module_id = m.id
                      WHERE m.name IN ({})",
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

                 let rows = stmt.query_map(rusqlite::params_from_iter(params.iter().cloned()), |row| {
                     Ok((
                         row.get::<_, String>(0)?, // name
                         row.get::<_, Option<String>>(1)?, // base
                         row.get::<_, i64>(2)?, // line
                         row.get::<_, String>(3)?, // path
                         row.get::<_, String>(4)?, // type
                     ))
                 })?;

                 for r in rows {
                     let (name, base, line, path, stype) = r?;
                     if symbol_type.is_some() {
                         let item = json!([name, line, stype, base.unwrap_or_default()]);
                         if !groups.contains_key(&path) { path_order.push(path.clone()); }
                         groups.entry(path).or_default().push(item);
                     } else {
                         all_results.push(json!([name, line, path, stype, base]));
                     }
                 }
             }

             if symbol_type.is_some() {
                 let res: Vec<Value> = path_order.into_iter().map(|path| {
                     let items = groups.remove(&path).unwrap();
                     json!({ "p": path, "i": items })
                 }).collect();
                 Ok(json!(res))
             } else {
                 Ok(json!(all_results))
             }
        },
        QueryRequest::GetRecursiveDerivedClasses { base_class } => {
             let mut stmt = conn.prepare(
                "WITH RECURSIVE derived_cte AS (
                  SELECT id, name, symbol_type FROM classes WHERE name = ?
                  UNION
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
                WHERE d.name != ?"
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
        },
        QueryRequest::GetRecursiveParentClasses { child_class } => {
              let mut stmt = conn.prepare(
                "WITH RECURSIVE parents_cte AS (
                  SELECT id, name, 0 as level FROM classes WHERE name = ?
                  UNION
                  SELECT p.id, p.name, c.level + 1
                  FROM classes p
                  JOIN inheritance i ON p.name = i.parent_name
                  JOIN parents_cte c ON i.child_id = c.id
                )
                SELECT d.name, '', c.line_number, f.path, f.filename, c.symbol_type, m.name, d.level
                FROM parents_cte d
                JOIN classes c ON d.id = c.id
                JOIN files f ON c.file_id = f.id
                JOIN modules m ON f.module_id = m.id
                ORDER BY d.level ASC"
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
        },
        QueryRequest::FindSymbolInInheritanceChain { class_name, symbol_name, mode } => {
             let is_impl = mode.unwrap_or_default() == "implementation";
             
             // 1. 継承チェーン内のクラスを近い順に取得
             // 2. 各クラスのメンバから symbol_name を探す
             // 3. 最初に見つかったものを返す
             let mut stmt = conn.prepare(
                "WITH RECURSIVE parents_cte AS (
                  SELECT id, name, 0 as level FROM classes WHERE name = ?
                  UNION
                  SELECT p.id, p.name, pc.level + 1
                  FROM classes p
                  JOIN inheritance i ON p.name = i.parent_name
                  JOIN parents_cte pc ON i.child_id = pc.id
                )
                SELECT f.path, m.line_number, p.name as class_name
                FROM parents_cte p
                JOIN members m ON p.id = m.class_id
                JOIN classes c ON p.id = c.id
                JOIN files f ON c.file_id = f.id
                WHERE m.name = ? AND p.level > 0
                ORDER BY p.level ASC
                LIMIT 1"
             )?;
             
             let res = stmt.query_row(params![class_name, symbol_name], |row| {
                 Ok(json!({
                     "file_path": row.get::<_, String>(0)?,
                     "line_number": row.get::<_, i64>(1)?,
                     "class_name": row.get::<_, String>(2)?,
                 }))
             }).optional()?;

             // もし実装(.cpp)を求めている場合
             if is_impl && res.is_some() {
                 let data = res.as_ref().unwrap();
                 let h_path = data["file_path"].as_str().unwrap();
                 let c_name = data["class_name"].as_str().unwrap();
                 
                 // ヘッダーのファイル名（拡張子なし）を取得
                 let h_stem = std::path::Path::new(h_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");

                 // .h に対応する .cpp を探す (同一モジュール内)
                 // ヘッダーのファイル名ステムで始まる .cpp を探す
                 let mut stmt_cpp = conn.prepare(
                     "SELECT f.path
                      FROM files f
                      WHERE f.module_id = (SELECT module_id FROM files WHERE path = ?)
                      AND f.extension IN ('cpp', 'c', 'cc')
                      AND f.filename LIKE ?
                      LIMIT 1"
                 )?;
                 let target_like = format!("{}%.cpp", h_stem);
                 
                 let res_cpp = stmt_cpp.query_row(params![h_path, target_like], |row| {
                     Ok(json!({
                         "file_path": row.get::<_, String>(0)?,
                         "line_number": 0, // Lua側でパターン検索させる
                         "class_name": c_name,
                     }))
                 }).optional()?;
                 
                 if res_cpp.is_some() {
                     return Ok(json!(res_cpp));
                 }
             }

             Ok(json!(res))
        },
        QueryRequest::GetVirtualFunctionsInInheritanceChain { class_name } => {
             let mut stmt = conn.prepare(
                "WITH RECURSIVE parents_cte AS (
                  SELECT id, name, 0 as level FROM classes WHERE name = ?
                  UNION
                  SELECT p.id, p.name, pc.level + 1
                  FROM classes p
                  JOIN inheritance i ON p.name = i.parent_name
                  JOIN parents_cte pc ON i.child_id = pc.id
                )
                SELECT m.name, m.type, m.flags, m.return_type, m.detail, m.line_number, f.path, p.name as class_name
                FROM parents_cte p
                JOIN members m ON p.id = m.class_id
                JOIN classes c ON p.id = c.id
                JOIN files f ON c.file_id = f.id
                WHERE m.flags LIKE '%virtual%'
                ORDER BY p.level ASC, m.name ASC"
             )?;
             
             let rows = stmt.query_map([class_name], |row| {
                 Ok(json!({
                     "name": row.get::<_, String>(0)?,
                     "kind": row.get::<_, String>(1)?,
                     "flags": row.get::<_, Option<String>>(2)?,
                     "return_type": row.get::<_, Option<String>>(3)?,
                     "params": row.get::<_, Option<String>>(4)?,
                     "line": row.get::<_, i64>(5)?,
                     "file_path": row.get::<_, String>(6)?,
                     "declared_in": row.get::<_, String>(7)?,
                     "is_virtual": true,
                 }))
             })?;
             
             let res: Result<Vec<Value>, _> = rows.collect();
             Ok(json!(res?))
        },
        QueryRequest::GetProgramFiles => {
             let mut stmt = conn.prepare(
                "SELECT f.path, m.name, m.root_path
                 FROM files f
                 JOIN modules m ON f.module_id = m.id
                 WHERE m.type = 'Program'"
             )?;
             let rows = stmt.query_map([], |row| {
                 Ok(json!({
                     "path": row.get::<_, String>(0)?,
                     "module_name": row.get::<_, String>(1)?,
                     "module_root": row.get::<_, String>(2)?,
                 }))
             })?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetAllIniFiles => {
             let mut stmt = conn.prepare(
                "SELECT f.path, m.name, m.root_path
                 FROM files f
                 JOIN modules m ON f.module_id = m.id
                 WHERE f.extension = 'ini'"
             )?;
             let rows = stmt.query_map([], |row| {
                 Ok(json!({
                     "path": row.get::<_, String>(0)?,
                     "module_name": row.get::<_, String>(1)?,
                     "module_root": row.get::<_, String>(2)?,
                 }))
             })?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::FindSymbolInModule { module, symbol } => {
             let mut stmt = conn.prepare(
                "SELECT f.path, c.line_number
                 FROM classes c
                 JOIN files f ON c.file_id = f.id
                 JOIN modules m ON f.module_id = m.id
                 WHERE m.name = ? AND c.name = ? LIMIT 1"
             )?;
             let res = stmt.query_row([&module, &symbol], |row| {
                 Ok(json!({
                     "file_path": row.get::<_, String>(0)?,
                     "line_number": row.get::<_, i64>(1)?,
                 }))
             }).optional()?;
             
             if let Some(r) = res {
                 return Ok(json!(r));
             }

             // Fallback to members search
             let mut stmt_mem = conn.prepare(
                "SELECT f.path, mem.line_number
                 FROM members mem
                 JOIN classes c ON mem.class_id = c.id
                 JOIN files f ON c.file_id = f.id
                 JOIN modules m ON f.module_id = m.id
                 WHERE m.name = ? AND mem.name = ? LIMIT 1"
             )?;
             
             let res_mem = stmt_mem.query_row([&module, &symbol], |row| {
                 Ok(json!({
                     "file_path": row.get::<_, String>(0)?,
                     "line_number": row.get::<_, i64>(1)?,
                 }))
             }).optional()?;
             
             Ok(json!(res_mem))
        },
        QueryRequest::FindClassByName { name } => {
             let mut stmt = conn.prepare(
                "SELECT c.id, c.name, c.base_class, c.line_number, f.path, f.filename, c.symbol_type, m.name, m.root_path
                 FROM classes c
                 JOIN files f ON c.file_id = f.id
                 JOIN modules m ON f.module_id = m.id
                 WHERE c.name = ? LIMIT 1"
             )?;
             let res = stmt.query_row([name], |row| {
                 Ok(json!({
                     "id": row.get::<_, i64>(0)?,
                     "class_name": row.get::<_, String>(1)?,
                     "base_class": row.get::<_, Option<String>>(2)?,
                     "line_number": row.get::<_, i64>(3)?,
                     "file_path": row.get::<_, String>(4)?,
                     "filename": row.get::<_, String>(5)?,
                     "symbol_type": row.get::<_, String>(6)?,
                     "module_name": row.get::<_, String>(7)?,
                     "module_root": row.get::<_, String>(8)?,
                 }))
             }).optional()?;
             Ok(json!(res))
        },
        QueryRequest::SearchClassesPrefix { prefix, limit } => {
             let mut stmt = conn.prepare(
                "SELECT name, symbol_type FROM classes WHERE name LIKE ? LIMIT ?"
             )?;
             let param = format!("{}%", prefix);
             let lim = limit.unwrap_or(50) as i64;
             let rows = stmt.query_map(params![param, lim], |row| {
                 Ok(json!({
                     "name": row.get::<_, String>(0)?,
                     "symbol_type": row.get::<_, String>(1)?,
                 }))
             })?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetClasses { extra_where, params: input_params } => {
             let mut sql = "SELECT c.id, c.name, c.base_class, c.symbol_type, f.path, m.name as module_name
                            FROM classes c
                            JOIN files f ON c.file_id = f.id
                            JOIN modules m ON f.module_id = m.id
                            WHERE c.symbol_type IN ('class', 'struct') AND c.name NOT LIKE '(%'".to_string();
             if let Some(w) = extra_where {
                 sql.push_str(" ");
                 sql.push_str(&w);
             }
             sql.push_str(" ORDER BY c.name ASC");
             
             let mut stmt = conn.prepare(&sql)?;
             let p = input_params.unwrap_or_default();
             let params_dyn: Vec<&dyn ToSql> = p.iter().map(|s| s as &dyn ToSql).collect();
             
             let rows = stmt.query_map(rusqlite::params_from_iter(params_dyn), |row| {
                 Ok(json!({
                     "id": row.get::<_, i64>(0)?,
                     "name": row.get::<_, String>(1)?,
                     "base_class": row.get::<_, Option<String>>(2)?,
                     "symbol_type": row.get::<_, String>(3)?,
                     "path": row.get::<_, String>(4)?,
                     "module_name": row.get::<_, String>(5)?,
                 }))
             })?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetStructs { extra_where, params: input_params } => {
             let mut sql = "SELECT c.id, c.name, c.base_class, c.symbol_type, f.path, m.name as module_name
                            FROM classes c
                            JOIN files f ON c.file_id = f.id
                            JOIN modules m ON f.module_id = m.id
                            WHERE c.symbol_type = 'struct' AND c.name NOT LIKE '(%'".to_string();
             if let Some(w) = extra_where {
                 sql.push_str(" ");
                 sql.push_str(&w);
             }
             sql.push_str(" ORDER BY c.name ASC");
             
             let mut stmt = conn.prepare(&sql)?;
             let p = input_params.unwrap_or_default();
             let params_dyn: Vec<&dyn ToSql> = p.iter().map(|s| s as &dyn ToSql).collect();
             
             let rows = stmt.query_map(rusqlite::params_from_iter(params_dyn), |row| {
                 Ok(json!({
                     "id": row.get::<_, i64>(0)?,
                     "name": row.get::<_, String>(1)?,
                     "base_class": row.get::<_, Option<String>>(2)?,
                     "symbol_type": row.get::<_, String>(3)?,
                     "path": row.get::<_, String>(4)?,
                     "module_name": row.get::<_, String>(5)?,
                 }))
             })?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetStructsOnly => {
              let mut stmt = conn.prepare(
                "SELECT c.id, c.name, c.base_class, c.symbol_type, f.path, m.name as module_name
                 FROM classes c
                 JOIN files f ON c.file_id = f.id
                 JOIN modules m ON f.module_id = m.id
                 WHERE c.symbol_type = 'struct' AND c.name NOT LIKE '(%'
                 ORDER BY c.name ASC"
             )?;
             let rows = stmt.query_map([], |row| {
                 Ok(json!({
                     "id": row.get::<_, i64>(0)?,
                     "name": row.get::<_, String>(1)?,
                     "base_class": row.get::<_, Option<String>>(2)?,
                     "symbol_type": row.get::<_, String>(3)?,
                     "path": row.get::<_, String>(4)?,
                     "module_name": row.get::<_, String>(5)?,
                 }))
             })?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetClassMembersById { class_id } => {
             let mut stmt = conn.prepare(
                "SELECT name, type, flags, access, detail, return_type, is_static
                 FROM members WHERE class_id = ? ORDER BY type, name"
             )?;
             let rows = stmt.query_map([class_id], |row| {
                 Ok(json!({
                     "name": row.get::<_, String>(0)?,
                     "type": row.get::<_, String>(1)?,
                     "flags": row.get::<_, Option<String>>(2)?,
                     "access": row.get::<_, Option<String>>(3)?,
                     "detail": row.get::<_, Option<String>>(4)?,
                     "return_type": row.get::<_, Option<String>>(5)?,
                     "is_static": row.get::<_, i64>(6)?,
                 }))
             })?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetClassMembers { class_name } => {
             let mut stmt = conn.prepare(
                "SELECT m.name, m.type, m.flags, m.access, m.detail, m.return_type, m.is_static
                 FROM members m JOIN classes c ON m.class_id = c.id
                 WHERE c.name = ? ORDER BY m.type, m.name"
             )?;
             let rows = stmt.query_map([class_name], |row| {
                 Ok(json!({
                     "name": row.get::<_, String>(0)?,
                     "type": row.get::<_, String>(1)?,
                     "flags": row.get::<_, Option<String>>(2)?,
                     "access": row.get::<_, Option<String>>(3)?,
                     "detail": row.get::<_, Option<String>>(4)?,
                     "return_type": row.get::<_, Option<String>>(5)?,
                     "is_static": row.get::<_, i64>(6)?,
                 }))
             })?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetClassMethods { class_name } => {
             let mut stmt = conn.prepare(
                "SELECT m.name, m.flags, m.access, m.detail, m.return_type, m.is_static
                 FROM members m JOIN classes c ON m.class_id = c.id
                 WHERE c.name = ? AND m.type = 'function' ORDER BY m.name"
             )?;
             let rows = stmt.query_map([class_name], |row| {
                 Ok(json!({
                     "name": row.get::<_, String>(0)?,
                     "flags": row.get::<_, Option<String>>(1)?,
                     "access": row.get::<_, Option<String>>(2)?,
                     "detail": row.get::<_, Option<String>>(3)?,
                     "return_type": row.get::<_, Option<String>>(4)?,
                     "is_static": row.get::<_, i64>(5)?,
                 }))
             })?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetClassProperties { class_name } => {
             let mut stmt = conn.prepare(
                "SELECT m.name, m.flags, m.access, m.detail, m.return_type, m.is_static
                 FROM members m JOIN classes c ON m.class_id = c.id
                 WHERE c.name = ? AND (m.type = 'variable' OR m.type = 'property') ORDER BY m.name"
             )?;
             let rows = stmt.query_map([class_name], |row| {
                 Ok(json!({
                     "name": row.get::<_, String>(0)?,
                     "flags": row.get::<_, Option<String>>(1)?,
                     "access": row.get::<_, Option<String>>(2)?,
                     "detail": row.get::<_, Option<String>>(3)?,
                     "return_type": row.get::<_, Option<String>>(4)?,
                     "is_static": row.get::<_, i64>(5)?,
                 }))
             })?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetClassMembersRecursive { class_name, namespace } => {
             let mut result_members = Vec::new();
             let mut visited = HashMap::new();
             let mut queue = vec![class_name.clone()];
             let initial_ns = namespace.clone();
             
             while let Some(current_name) = queue.pop() {
                 let (search_name, search_ns) = if let Some(idx) = current_name.find("::") {
                     let ns = &current_name[0..idx];
                     let name = &current_name[idx+2..];
                     (name.to_string(), Some(ns.to_string()))
                 } else {
                     if queue.len() == 0 { // First item
                         (current_name.clone(), initial_ns.clone())
                     } else {
                         (current_name.clone(), None)
                     }
                 };

                 let visited_key = format!("{}::{}", search_ns.clone().unwrap_or_default(), search_name);
                 if visited.contains_key(&visited_key) { continue; }
                 visited.insert(visited_key, true);

                 let mut stmt = conn.prepare(
                     "SELECT c.id, c.symbol_type 
                      FROM classes c
                      JOIN files f ON c.file_id = f.id
                      WHERE c.name = ? 
                      ORDER BY 
                        (CASE 
                          WHEN c.namespace = ? THEN 0 
                          WHEN f.path LIKE '%/Runtime/Core/%' THEN 1
                          WHEN f.path LIKE '%/Runtime/Engine/%' THEN 2
                          WHEN c.namespace IS NULL OR c.namespace = '' THEN 3
                          ELSE 4 END) ASC,
                        (CASE WHEN c.symbol_type = 'UCLASS' THEN 0 WHEN c.symbol_type = 'USTRUCT' THEN 1 ELSE 2 END) ASC"
                 )?;
                 let ns_param = search_ns.clone().unwrap_or_default();
                 let row = stmt.query_row(params![search_name, ns_param], |row| {
                     Ok(row.get::<_, i64>(0)?)
                 }).optional()?;

                 if let Some(class_id) = row {
                     // Members
                     let mut mem_stmt = conn.prepare(
                         "SELECT name, type, flags, access, detail, return_type, is_static
                          FROM members WHERE class_id = ? ORDER BY type, name"
                     )?;
                     let mem_iter = mem_stmt.query_map([class_id], |row| {
                         Ok(json!({
                             "name": row.get::<_, String>(0)?,
                             "type": row.get::<_, String>(1)?,
                             "flags": row.get::<_, Option<String>>(2)?,
                             "access": row.get::<_, Option<String>>(3)?,
                             "detail": row.get::<_, Option<String>>(4)?,
                             "return_type": row.get::<_, Option<String>>(5)?,
                             "is_static": row.get::<_, i64>(6)?,
                             "class_name": search_name.clone(),
                         }))
                     })?;
                     
                     for m in mem_iter {
                         let m = m?;
                         let name = m["name"].as_str().unwrap();
                         if !result_members.iter().any(|existing: &Value| existing["name"].as_str() == Some(name)) {
                             result_members.push(m);
                         }
                     }

                     // Enum values
                     let mut enum_stmt = conn.prepare("SELECT name FROM enum_values WHERE enum_id = ?")?;
                     let enum_iter = enum_stmt.query_map([class_id], |row| {
                         Ok(json!({
                             "name": row.get::<_, String>(0)?,
                             "type": "enum_item",
                             "flags": "",
                             "detail": "",
                             "return_type": "",
                             "is_static": 0,
                             "access": "public",
                             "class_name": search_name.clone(),
                         }))
                     })?;
                     for e in enum_iter { result_members.push(e?); }

                     // Parents
                     let mut parent_stmt = conn.prepare(
                         "SELECT parent_name FROM inheritance WHERE child_id = ? AND parent_name != ?"
                     )?;
                     let parents = parent_stmt.query_map(params![class_id, search_name], |row| {
                         Ok(row.get::<_, String>(0)?)
                     })?;
                     for p in parents {
                         queue.push(p?);
                     }
                 }
             }
             Ok(json!(result_members))
        },
        QueryRequest::SearchFilesByPathPart { part } => {
             let mut stmt = conn.prepare(
                "SELECT f.path, f.filename, m.root_path 
                 FROM files f JOIN modules m ON f.module_id = m.id
                 WHERE f.path LIKE ? LIMIT 50"
             )?;
             let param = format!("%{}%", part);
             let rows = stmt.query_map([param], |row| {
                 Ok(json!({
                     "path": row.get::<_, String>(0)?,
                     "filename": row.get::<_, String>(1)?,
                     "module_root": row.get::<_, String>(2)?,
                 }))
             })?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetEnumValues { enum_name } => {
             let mut stmt = conn.prepare(
                "SELECT ev.name FROM enum_values ev JOIN classes c ON ev.enum_id = c.id
                 WHERE c.name = ? AND c.symbol_type = 'enum'"
             )?;
             let rows = stmt.query_map([enum_name], |row| Ok(json!(row.get::<_, String>(0)?)))?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetComponents => {
             let mut stmt = conn.prepare("SELECT * FROM components ORDER BY name ASC")?;
             let rows = stmt.query_map([], |row| {
                 Ok(json!({
                     "id": row.get::<_, i64>("id")?,
                     "name": row.get::<_, String>("name")?,
                     "display_name": row.get::<_, Option<String>>("display_name")?,
                     "type": row.get::<_, Option<String>>("type")?,
                     "owner_name": row.get::<_, Option<String>>("owner_name")?,
                     "root_path": row.get::<_, Option<String>>("root_path")?,
                     "uplugin_path": row.get::<_, Option<String>>("uplugin_path")?,
                     "uproject_path": row.get::<_, Option<String>>("uproject_path")?,
                     "engine_association": row.get::<_, Option<String>>("engine_association")?,
                 }))
             })?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetModules => {
             let mut stmt = conn.prepare("SELECT * FROM modules ORDER BY name ASC")?;
             let rows = stmt.query_map([], |row| {
                 Ok(json!({
                     "id": row.get::<_, i64>("id")?,
                     "name": row.get::<_, String>("name")?,
                     "type": row.get::<_, Option<String>>("type")?,
                     "scope": row.get::<_, Option<String>>("scope")?,
                     "root_path": row.get::<_, String>("root_path")?,
                     "build_cs_path": row.get::<_, Option<String>>("build_cs_path")?,
                     "owner_name": row.get::<_, Option<String>>("owner_name")?,
                     "component_name": row.get::<_, Option<String>>("component_name")?,
                     "deep_dependencies": row.get::<_, Option<String>>("deep_dependencies")?,
                 }))
             })?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetModuleIdByName { name } => {
             let mut stmt = conn.prepare("SELECT id FROM modules WHERE name = ?")?;
             let res = stmt.query_row([name], |row| Ok(row.get::<_, i64>(0)?)).optional()?;
             Ok(json!(res))
        },
        QueryRequest::GetModuleRootPath { name } => {
             let mut stmt = conn.prepare("SELECT root_path FROM modules WHERE name = ?")?;
             let res = stmt.query_row([name], |row| Ok(row.get::<_, String>(0)?)).optional()?;
             Ok(json!(res))
        },
        QueryRequest::GetFilesInModule { module_id } => {
             let mut stmt = conn.prepare("SELECT path FROM files WHERE module_id = ?")?;
             let rows = stmt.query_map([module_id], |row| Ok(json!(row.get::<_, String>(0)?)))?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetFilesInModules { modules, extensions, filter } => {
             if modules.is_empty() { return Ok(json!([])); }
             
             let mut all_files = Vec::new();
             for chunk in modules.chunks(500) {
                 let placeholders: Vec<String> = chunk.iter().map(|_| "?".to_string()).collect();
                 let mut sql = format!(
                     "SELECT f.path, f.extension, m.name, m.root_path
                      FROM files f
                      JOIN modules m ON f.module_id = m.id
                      WHERE m.name IN ({})",
                     placeholders.join(",")
                 );

                 if let Some(exts) = &extensions {
                     if !exts.is_empty() {
                         let ext_placeholders: Vec<String> = exts.iter().map(|_| "?".to_string()).collect();
                         sql.push_str(&format!(" AND f.extension IN ({})", ext_placeholders.join(",")));
                     }
                 }
                 if filter.is_some() {
                     sql.push_str(" AND f.path LIKE ?");
                 }
                 
                 let mut stmt = conn.prepare(&sql)?;
                 let mut params: Vec<&dyn ToSql> = chunk.iter().map(|s| s as &dyn ToSql).collect();
                 
                 if let Some(exts) = &extensions {
                     for ext in exts { params.push(ext); }
                 }
                 if let Some(f) = &filter {
                     params.push(f);
                 }

                 let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
                     Ok(json!({
                         "file_path": row.get::<_, String>(0)?,
                         "extension": row.get::<_, String>(1)?,
                         "module_name": row.get::<_, String>(2)?,
                         "module_root": row.get::<_, String>(3)?,
                     }))
                 })?;
                 for r in rows { all_files.push(r?); }
             }
             Ok(json!(all_files))
        },
        QueryRequest::SearchFilesInModules { modules, filter, limit } => {
             if modules.is_empty() { return Ok(json!([])); }
             let limit_val = limit.unwrap_or(100);
             let mut all_files = Vec::new();
             
             for chunk in modules.chunks(500) {
                 if all_files.len() >= limit_val { break; }
                 
                 let remaining = limit_val - all_files.len();
                 let placeholders: Vec<String> = chunk.iter().map(|_| "?".to_string()).collect();
                 let sql = format!(
                     "SELECT f.path, f.extension, m.name, m.root_path
                      FROM files f
                      JOIN modules m ON f.module_id = m.id
                      WHERE m.name IN ({}) AND f.path LIKE ? LIMIT ?",
                     placeholders.join(",")
                 );
                 
                 let filter_param = format!("%{}%", filter);
                 let mut params: Vec<&dyn ToSql> = chunk.iter().map(|s| s as &dyn ToSql).collect();
                 params.push(&filter_param);
                 let limit_param = remaining as i64;
                 params.push(&limit_param);
                 
                 let mut stmt = conn.prepare(&sql)?;
                 let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
                     Ok(json!({
                         "file_path": row.get::<_, String>(0)?,
                         "extension": row.get::<_, String>(1)?,
                         "module_name": row.get::<_, String>(2)?,
                         "module_root": row.get::<_, String>(3)?,
                     }))
                 })?;
                 
                 for r in rows { 
                    all_files.push(r?);
                 }
             }
             Ok(json!(all_files))
        },
        QueryRequest::SearchSymbolsInModules { modules, symbol_type, filter, limit } => {
             if modules.is_empty() { return Ok(json!([])); }
             let limit_val = limit.unwrap_or(100);
             let mut all_results = Vec::new();

             for chunk in modules.chunks(500) {
                 if all_results.len() >= limit_val { break; }

                 let remaining = limit_val - all_results.len();
                 let placeholders: Vec<String> = chunk.iter().map(|_| "?".to_string()).collect();
                 
                 let mut sql = format!(
                     "SELECT c.name, c.base_class, c.line_number, f.path, c.symbol_type, m.name
                      FROM classes c
                      JOIN files f ON c.file_id = f.id
                      JOIN modules m ON f.module_id = m.id
                      WHERE m.name IN ({}) AND c.name LIKE ?",
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
                 
                 sql.push_str(" LIMIT ?");

                 let filter_param = format!("%{}%", filter);
                 let mut params: Vec<&dyn ToSql> = chunk.iter().map(|s| s as &dyn ToSql).collect();
                 params.push(&filter_param);
                 let limit_param = remaining as i64;
                 params.push(&limit_param);

                 let mut stmt = conn.prepare(&sql)?;
                 let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
                     Ok(json!({
                         "name": row.get::<_, String>(0)?,
                         "base_class": row.get::<_, Option<String>>(1)?,
                         "line_number": row.get::<_, i64>(2)?,
                         "path": row.get::<_, String>(3)?,
                         "symbol_type": row.get::<_, String>(4)?,
                         "module_name": row.get::<_, String>(5)?,
                     }))
                 })?;

                 for r in rows {
                     all_results.push(r?);
                 }
             }
             Ok(json!(all_results))
        },
        QueryRequest::GetDirectoriesInModule { module_id: _ } => {
             Ok(json!([]))
        },
        QueryRequest::GetModuleFilesByNameAndRoot { name, root } => {
             let mut stmt = conn.prepare(
                "SELECT f.path, f.extension FROM files f JOIN modules m ON f.module_id = m.id
                 WHERE m.name = ? AND m.root_path = ?"
             )?;
             let rows = stmt.query_map([name, root], |row| {
                 Ok(json!({
                     "path": row.get::<_, String>(0)?,
                     "extension": row.get::<_, String>(1)?,
                 }))
             })?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetModuleDirsByNameAndRoot { name: _, root: _ } => {
             Ok(json!([]))
        },
        QueryRequest::GetClassFilePath { class_name } => {
             let mut stmt = conn.prepare(
                "SELECT f.path FROM files f JOIN classes c ON c.file_id = f.id WHERE c.name = ? LIMIT 1"
             )?;
             let res = stmt.query_row([class_name], |row| Ok(row.get::<_, String>(0)?)).optional()?;
             Ok(json!(res))
        },
        QueryRequest::GetFileSymbols { file_path } => {
            // 1. Get all classes/structs/enums in this file
            let mut stmt = conn.prepare(
                "SELECT c.id, c.name, c.symbol_type, c.line_number, c.namespace, c.base_class
                 FROM classes c JOIN files f ON c.file_id = f.id
                 WHERE f.path = ?"
            )?;
            let class_rows = stmt.query_map([&file_path], |row| {
                Ok((
                    row.get::<_, i64>(0)?,    // id
                    row.get::<_, String>(1)?, // name
                    row.get::<_, String>(2)?, // type
                    row.get::<_, i64>(3)?,    // line
                    row.get::<_, Option<String>>(4)?, // namespace
                    row.get::<_, Option<String>>(5)?, // base
                ))
            })?;

            let mut results = Vec::new();
            for r in class_rows {
                let (cid, cname, ctype, cline, cns, cbase) = r?;
                
                // kind を UI が期待するキャメルケースに変換
                let mapped_class_kind = match ctype.as_str() {
                    "UCLASS" | "class" => "UClass",
                    "USTRUCT" | "struct" => "UStruct",
                    "UENUM" | "enum" => "UEnum",
                    _ => &ctype,
                };

                let mut class_info = json!({
                    "name": cname,
                    "kind": mapped_class_kind,
                    "line": cline,
                    "namespace": cns,
                    "base_class": cbase,
                    "file_path": file_path,
                    "fields": { "public": [], "protected": [], "private": [] },
                    "methods": { "public": [], "protected": [], "private": [] }
                });

                // 2. Get members for this class
                let mut mem_stmt = conn.prepare(
                    "SELECT name, type, flags, access, detail, return_type, is_static, line_number
                     FROM members WHERE class_id = ? ORDER BY name"
                )?;
                let mem_rows = mem_stmt.query_map([cid], |row| {
                    let m_name: String = row.get(0)?;
                    let m_type: String = row.get(1)?;
                    let m_flags: Option<String> = row.get(2)?;
                    let m_access: Option<String> = row.get(3)?;
                    let m_detail: Option<String> = row.get(4)?;
                    let m_return_type: Option<String> = row.get(5)?;
                    let is_static: bool = row.get::<_, i64>(6)? == 1;
                    let line: i64 = row.get(7)?;

                    let flags_str = m_flags.clone().unwrap_or_default();
                    let mapped_kind = if m_type == "function" {
                        if flags_str.contains("UFUNCTION") { "UFunction" } else { "Function" }
                    } else if m_type == "property" || m_type == "variable" {
                        if flags_str.contains("UPROPERTY") { "UProperty" } else { "Field" }
                    } else if m_type == "enum_item" {
                        "EnumItem"
                    } else {
                        &m_type
                    };

                    Ok(json!({
                        "name": m_name,
                        "kind": mapped_kind,
                        "flags": m_flags,
                        "access": m_access,
                        "detail": m_detail,
                        "return_type": m_return_type,
                        "is_static": is_static,
                        "file_path": file_path,
                        "line": line,
                    }))
                })?;

                for m_res in mem_rows {
                    let m = m_res?;
                    let access = m["access"].as_str().unwrap_or("public").to_lowercase();
                    let m_kind = m["kind"].as_str().unwrap_or("");
                    let target_map = if m_kind.to_lowercase().contains("function") { "methods" } else { "fields" };
                    if let Some(target) = class_info[target_map].as_object_mut() {
                        target.entry(access).or_insert(json!([])).as_array_mut().unwrap().push(m);
                    }
                }

                // 3. Get enum values if it's an enum
                if ctype == "enum" || ctype == "UENUM" {
                    let mut enum_stmt = conn.prepare("SELECT name FROM enum_values WHERE enum_id = ?")?;
                    let ev_rows = enum_stmt.query_map([cid], |row| {
                        Ok(json!({
                            "name": row.get::<_, String>(0)?,
                            "kind": "EnumItem",
                            "file_path": file_path,
                            "line": cline
                        }))
                    })?;
                    for ev in ev_rows {
                        class_info["fields"]["public"].as_array_mut().unwrap().push(ev?);
                    }
                }

                results.push(class_info);
            }
            Ok(json!(results))
        },
        QueryRequest::UpdateMemberReturnType { class_name, member_name, return_type } => {
             let mut stmt = conn.prepare(
                "UPDATE members SET return_type = ? 
                 WHERE name = ? AND class_id = (SELECT id FROM classes WHERE name = ?)"
             )?;
             let count = stmt.execute(params![return_type, member_name, class_name])?;
             Ok(json!({ "updated": count }))
        },
        QueryRequest::GetTargetFiles => {
             let mut stmt = conn.prepare("SELECT path, filename FROM files WHERE filename LIKE '%.Target.cs'")?;
             let rows = stmt.query_map([], |row| {
                 Ok(json!({
                     "path": row.get::<_, String>(0)?,
                     "filename": row.get::<_, String>(1)?,
                 }))
             })?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetAllFilePaths => {
             let mut stmt = conn.prepare("SELECT path FROM files")?;
             let rows = stmt.query_map([], |row| Ok(json!(row.get::<_, String>(0)?)))?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetAllFilesMetadata => {
             let mut stmt = conn.prepare(
                "SELECT f.filename, f.path, m.name
                 FROM files f JOIN modules m ON f.module_id = m.id"
             )?;
             let rows = stmt.query_map([], |row| {
                 Ok(json!({
                     "filename": row.get::<_, String>(0)?,
                     "path": row.get::<_, String>(1)?,
                     "module_name": row.get::<_, String>(2)?,
                 }))
             })?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        }
    }
}