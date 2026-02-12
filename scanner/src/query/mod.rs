use rusqlite::{Connection, ToSql, OptionalExtension};
use serde_json::{json, Value};
use crate::types::QueryRequest;

pub mod asset;
pub mod class;
pub mod module;
pub mod buffer;

pub fn process_query_streaming<F>(conn: &Connection, req: QueryRequest, mut on_items: F) -> anyhow::Result<Value> 
where F: FnMut(Vec<Value>) -> anyhow::Result<()> {
    match req {
        QueryRequest::GrepAssets { pattern, .. } => asset::grep_assets(conn, pattern, on_items),
        QueryRequest::GetFilesInModulesAsync { modules, extensions, filter } => {
            if modules.is_empty() { return Ok(json!(0)); }
            let mut total_count = 0;
            for chunk in modules.chunks(500) {
                let placeholders: Vec<String> = chunk.iter().map(|_| "?".to_string()).collect();
                let mut sql = format!(
                    "SELECT sp.text as path, f.extension, sm.text as name, sr.text as root_path 
                     FROM files f 
                     JOIN strings sp ON f.path_id = sp.id
                     JOIN modules m ON f.module_id = m.id 
                     JOIN strings sm ON m.name_id = sm.id
                     JOIN strings sr ON m.root_path_id = sr.id
                     WHERE sm.text IN ({})", 
                    placeholders.join(",")
                );
                if let Some(exts) = &extensions { if !exts.is_empty() { let ext_placeholders: Vec<String> = exts.iter().map(|_| "?".to_string()).collect(); sql.push_str(&format!(" AND f.extension IN ({})", ext_placeholders.join(","))); } }
                if filter.is_some() { sql.push_str(" AND sp.text LIKE ?"); }
                let mut stmt = conn.prepare(&sql)?;
                let mut params: Vec<&dyn ToSql> = chunk.iter().map(|s| s as &dyn ToSql).collect();
                if let Some(exts) = &extensions { for ext in exts { params.push(ext); } }
                if let Some(f) = &filter { params.push(f); }
                let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| Ok(json!({ "file_path": row.get::<_, String>(0)?, "extension": row.get::<_, String>(1)?, "module_name": row.get::<_, String>(2)?, "module_root": row.get::<_, String>(3)? })))?;
                let mut batch = Vec::new();
                for r in rows { batch.push(r?); if batch.len() >= 1000 { total_count += batch.len(); on_items(batch)?; batch = Vec::new(); } }
                if !batch.is_empty() { total_count += batch.len(); on_items(batch)?; }
            }
            Ok(json!(total_count))
        },
        QueryRequest::SearchFilesInModulesAsync { modules, filter, limit } => {
            if modules.is_empty() { return Ok(json!(0)); }
            let limit_val = limit.unwrap_or(usize::MAX);
            let mut total_count = 0;
            for chunk in modules.chunks(500) {
                if total_count >= limit_val { break; }
                let remaining = limit_val - total_count;
                let placeholders: Vec<String> = chunk.iter().map(|_| "?".to_string()).collect();
                let sql = format!(
                    "SELECT sp.text as path, f.extension, sm.text as name, sr.text as root_path 
                     FROM files f 
                     JOIN strings sp ON f.path_id = sp.id
                     JOIN modules m ON f.module_id = m.id 
                     JOIN strings sm ON m.name_id = sm.id
                     JOIN strings sr ON m.root_path_id = sr.id
                     WHERE sm.text IN ({}) AND sp.text LIKE ? LIMIT ?", 
                    placeholders.join(",")
                );
                let filter_param = format!("%{}%", filter);
                let mut params: Vec<&dyn ToSql> = chunk.iter().map(|s| s as &dyn ToSql).collect();
                params.push(&filter_param);
                let limit_param = remaining as i64;
                params.push(&limit_param);
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| Ok(json!({ "file_path": row.get::<_, String>(0)?, "extension": row.get::<_, String>(1)?, "module_name": row.get::<_, String>(2)?, "module_root": row.get::<_, String>(3)? })))?;
                let mut batch = Vec::new();
                for r in rows { batch.push(r?); if batch.len() >= 500 { total_count += batch.len(); on_items(batch)?; batch = Vec::new(); } }
                if !batch.is_empty() { total_count += batch.len(); on_items(batch)?; }
            }
            Ok(json!(total_count))
        },
        QueryRequest::GetClassesInModulesAsync { modules, symbol_type } => {
            if modules.is_empty() { return Ok(json!(0)); }
            let mut total_count = 0;
            for chunk in modules.chunks(500) {
                let placeholders: Vec<String> = chunk.iter().map(|_| "?".to_string()).collect();
                let mut sql = format!(
                    "SELECT sc.text as name, sb.text as base_class, c.line_number, sp.text as path, c.symbol_type, sm.text as module_name 
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
                if let Some(st) = &symbol_type { match st.as_str() { "class" => sql.push_str(" AND (c.symbol_type = 'class' OR c.symbol_type = 'UCLASS' OR c.symbol_type = 'UINTERFACE')"), "struct" => sql.push_str(" AND (c.symbol_type = 'struct' OR c.symbol_type = 'USTRUCT')"), "enum" => sql.push_str(" AND (c.symbol_type = 'enum' OR c.symbol_type = 'UENUM')"), _ => sql.push_str(&format!(" AND c.symbol_type = '{}'", st)), } }
                let mut stmt = conn.prepare(&sql)?;
                let params: Vec<&dyn ToSql> = chunk.iter().map(|s| s as &dyn ToSql).collect();
                let rows = stmt.query_map(rusqlite::params_from_iter(params.iter().cloned()), |row| Ok(json!({ "name": row.get::<_, String>(0)?, "base": row.get::<_, Option<String>>(1)?, "line": row.get::<_, i64>(2)?, "path": row.get::<_, String>(3)?, "type": row.get::<_, String>(4)?, "module": row.get::<_, String>(5)? })))?;
                let mut batch = Vec::new();
                for r in rows { batch.push(r?); if batch.len() >= 1000 { total_count += batch.len(); on_items(batch)?; batch = Vec::new(); } }
                if !batch.is_empty() { total_count += batch.len(); on_items(batch)?; }
            }
            Ok(json!(total_count))
        },
        _ => process_query(conn, req)
    }
}

pub fn process_query(conn: &Connection, req: QueryRequest) -> anyhow::Result<Value> {
    match req {
        QueryRequest::FindDerivedClasses { base_class } => class::find_derived_classes(conn, base_class),
        QueryRequest::SearchFiles { part } => asset::search_files(conn, part),
        QueryRequest::LoadComponentData { component } => module::load_component_data(conn, component),
        QueryRequest::GetModuleByName { name } => module::get_module_by_name(conn, name),
        QueryRequest::GetClassesInModules { modules, symbol_type } => class::get_classes_in_modules(conn, modules, symbol_type),
        QueryRequest::GetRecursiveDerivedClasses { base_class } => class::get_recursive_derived_classes(conn, base_class),
        QueryRequest::GetRecursiveParentClasses { child_class } => class::get_recursive_parent_classes(conn, child_class),
        QueryRequest::FindSymbolInInheritanceChain { class_name, symbol_name, mode } => class::find_symbol_in_inheritance_chain(conn, class_name, symbol_name, mode),
        QueryRequest::GetVirtualFunctionsInInheritanceChain { class_name } => class::get_virtual_functions_in_inheritance_chain(conn, class_name),
        QueryRequest::GetProgramFiles => {
             let mut stmt = conn.prepare(
                "SELECT sp.text as path, sm.text as name, sr.text as root_path 
                 FROM files f 
                 JOIN strings sp ON f.path_id = sp.id
                 JOIN modules m ON f.module_id = m.id 
                 JOIN strings sm ON m.name_id = sm.id
                 JOIN strings sr ON m.root_path_id = sr.id
                 WHERE m.type = 'Program'"
             )?;
             let rows = stmt.query_map([], |row| Ok(json!({ "path": row.get::<_, String>(0)?, "module_name": row.get::<_, String>(1)?, "module_root": row.get::<_, String>(2)? })))?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetAllIniFiles => {
             let mut stmt = conn.prepare(
                "SELECT sp.text as path, sm.text as name, sr.text as root_path 
                 FROM files f 
                 JOIN strings sp ON f.path_id = sp.id
                 JOIN modules m ON f.module_id = m.id 
                 JOIN strings sm ON m.name_id = sm.id
                 JOIN strings sr ON m.root_path_id = sr.id
                 WHERE f.extension = 'ini'"
             )?;
             let rows = stmt.query_map([], |row| Ok(json!({ "path": row.get::<_, String>(0)?, "module_name": row.get::<_, String>(1)?, "module_root": row.get::<_, String>(2)? })))?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::FindSymbolInModule { module, symbol } => {
             let mut stmt = conn.prepare(
                "SELECT sp.text as path, c.line_number 
                 FROM classes c 
                 JOIN strings sc ON c.name_id = sc.id
                 JOIN files f ON c.file_id = f.id 
                 JOIN strings sp ON f.path_id = sp.id
                 JOIN modules m ON f.module_id = m.id 
                 JOIN strings sm ON m.name_id = sm.id
                 WHERE sm.text = ? AND sc.text = ? LIMIT 1"
             )?;
             if let Some(r) = stmt.query_row([&module, &symbol], |row| Ok(json!({ "file_path": row.get::<_, String>(0)?, "line_number": row.get::<_, i64>(1)? }))).optional()? { return Ok(json!(r)); }
             let mut stmt_mem = conn.prepare(
                "SELECT sp.text as path, mem.line_number 
                 FROM members mem 
                 JOIN strings smem ON mem.name_id = smem.id
                 JOIN classes c ON mem.class_id = c.id 
                 JOIN files f ON c.file_id = f.id 
                 JOIN strings sp ON f.path_id = sp.id
                 JOIN modules m ON f.module_id = m.id 
                 JOIN strings sm ON m.name_id = sm.id
                 WHERE sm.text = ? AND smem.text = ? LIMIT 1"
             )?;
             let res_mem = stmt_mem.query_row([&module, &symbol], |row| Ok(json!({ "file_path": row.get::<_, String>(0)?, "line_number": row.get::<_, i64>(1)? }))).optional()?;
             Ok(json!(res_mem))
        },
        QueryRequest::FindClassByName { name } => class::find_class_by_name(conn, name),
        QueryRequest::SearchClassesPrefix { prefix, limit } => class::search_classes_prefix(conn, prefix, limit),
        QueryRequest::GetClasses { extra_where, params: input_params } => {
             let mut sql = "SELECT c.id, sc.text as name, sb.text as base_class, c.symbol_type, sp.text as path, sm.text as module_name 
                            FROM classes c 
                            JOIN strings sc ON c.name_id = sc.id
                            LEFT JOIN strings sb ON c.base_class_id = sb.id
                            JOIN files f ON c.file_id = f.id 
                            JOIN strings sp ON f.path_id = sp.id
                            JOIN modules m ON f.module_id = m.id 
                            JOIN strings sm ON m.name_id = sm.id
                            WHERE c.symbol_type IN ('class', 'struct') AND sc.text NOT LIKE '(%'".to_string();
             if let Some(w) = extra_where { sql.push_str(" "); sql.push_str(&w); }
             sql.push_str(" ORDER BY sc.text ASC");
             let mut stmt = conn.prepare(&sql)?;
             let p = input_params.unwrap_or_default();
             let params_dyn: Vec<&dyn ToSql> = p.iter().map(|s| s as &dyn ToSql).collect();
             let rows = stmt.query_map(rusqlite::params_from_iter(params_dyn), |row| Ok(json!({ "id": row.get::<_, i64>(0)?, "name": row.get::<_, String>(1)?, "base_class": row.get::<_, Option<String>>(2)?, "symbol_type": row.get::<_, String>(3)?, "path": row.get::<_, String>(4)?, "module_name": row.get::<_, String>(5)? })))?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetStructs { extra_where, params: input_params } => {
             let mut sql = "SELECT c.id, sc.text as name, sb.text as base_class, c.symbol_type, sp.text as path, sm.text as module_name 
                            FROM classes c 
                            JOIN strings sc ON c.name_id = sc.id
                            LEFT JOIN strings sb ON c.base_class_id = sb.id
                            JOIN files f ON c.file_id = f.id 
                            JOIN strings sp ON f.path_id = sp.id
                            JOIN modules m ON f.module_id = m.id 
                            JOIN strings sm ON m.name_id = sm.id
                            WHERE c.symbol_type = 'struct' AND sc.text NOT LIKE '(%'".to_string();
             if let Some(w) = extra_where { sql.push_str(" "); sql.push_str(&w); }
             sql.push_str(" ORDER BY sc.text ASC");
             let mut stmt = conn.prepare(&sql)?;
             let p = input_params.unwrap_or_default();
             let params_dyn: Vec<&dyn ToSql> = p.iter().map(|s| s as &dyn ToSql).collect();
             let rows = stmt.query_map(rusqlite::params_from_iter(params_dyn), |row| Ok(json!({ "id": row.get::<_, i64>(0)?, "name": row.get::<_, String>(1)?, "base_class": row.get::<_, Option<String>>(2)?, "symbol_type": row.get::<_, String>(3)?, "path": row.get::<_, String>(4)?, "module_name": row.get::<_, String>(5)? })))?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetStructsOnly => {
              let mut stmt = conn.prepare("SELECT c.id, sc.text as name, sb.text as base_class, c.symbol_type, sp.text as path, sm.text as module_name FROM classes c JOIN strings sc ON c.name_id = sc.id LEFT JOIN strings sb ON c.base_class_id = sb.id JOIN files f ON c.file_id = f.id JOIN strings sp ON f.path_id = sp.id JOIN modules m ON f.module_id = m.id JOIN strings sm ON m.name_id = sm.id WHERE c.symbol_type = 'struct' AND sc.text NOT LIKE '(%' ORDER BY sc.text ASC")?;
              let rows = stmt.query_map([], |row| Ok(json!({ "id": row.get::<_, i64>(0)?, "name": row.get::<_, String>(1)?, "base_class": row.get::<_, Option<String>>(2)?, "symbol_type": row.get::<_, String>(3)?, "path": row.get::<_, String>(4)?, "module_name": row.get::<_, String>(5)? })))?;
              Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetClassMembersById { class_id } => {
             let mut stmt = conn.prepare("SELECT sn.text as name, st.text as type, flags, access, detail, srt.text as return_type, is_static FROM members m JOIN strings sn ON m.name_id = sn.id JOIN strings st ON m.type_id = st.id LEFT JOIN strings srt ON m.return_type_id = srt.id WHERE class_id = ? ORDER BY st.text, sn.text")?;
             let rows = stmt.query_map([class_id], |row| Ok(json!({ "name": row.get::<_, String>(0)?, "type": row.get::<_, String>(1)?, "flags": row.get::<_, Option<String>>(2)?, "access": row.get::<_, Option<String>>(3)?, "detail": row.get::<_, Option<String>>(4)?, "return_type": row.get::<_, Option<String>>(5)?, "is_static": row.get::<_, i64>(6)? })))?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetClassMembers { class_name } => {
             let mut stmt = conn.prepare("SELECT sn.text as name, st.text as type, m.flags, m.access, m.detail, srt.text as return_type, m.is_static FROM members m JOIN strings sn ON m.name_id = sn.id JOIN strings st ON m.type_id = st.id LEFT JOIN strings srt ON m.return_type_id = srt.id JOIN classes c ON m.class_id = c.id JOIN strings sc ON c.name_id = sc.id WHERE sc.text = ? ORDER BY st.text, sn.text")?;
             let rows = stmt.query_map([class_name], |row| Ok(json!({ "name": row.get::<_, String>(0)?, "type": row.get::<_, String>(1)?, "flags": row.get::<_, Option<String>>(2)?, "access": row.get::<_, Option<String>>(3)?, "detail": row.get::<_, Option<String>>(4)?, "return_type": row.get::<_, Option<String>>(5)?, "is_static": row.get::<_, i64>(6)? })))?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetClassMethods { class_name } => {
             let mut stmt = conn.prepare("SELECT sn.text as name, m.flags, m.access, m.detail, srt.text as return_type, m.is_static FROM members m JOIN strings sn ON m.name_id = sn.id JOIN strings st ON m.type_id = st.id LEFT JOIN strings srt ON m.return_type_id = srt.id JOIN classes c ON m.class_id = c.id JOIN strings sc ON c.name_id = sc.id WHERE sc.text = ? AND st.text = 'function' ORDER BY sn.text")?;
             let rows = stmt.query_map([class_name], |row| Ok(json!({ "name": row.get::<_, String>(0)?, "flags": row.get::<_, Option<String>>(1)?, "access": row.get::<_, Option<String>>(2)?, "detail": row.get::<_, Option<String>>(3)?, "return_type": row.get::<_, Option<String>>(4)?, "is_static": row.get::<_, i64>(5)? })))?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetClassProperties { class_name } => {
             let mut stmt = conn.prepare("SELECT sn.text as name, m.flags, m.access, m.detail, srt.text as return_type, m.is_static FROM members m JOIN strings sn ON m.name_id = sn.id JOIN strings st ON m.type_id = st.id LEFT JOIN strings srt ON m.return_type_id = srt.id JOIN classes c ON m.class_id = c.id JOIN strings sc ON c.name_id = sc.id WHERE sc.text = ? AND (st.text = 'variable' OR st.text = 'property') ORDER BY sn.text")?;
             let rows = stmt.query_map([class_name], |row| Ok(json!({ "name": row.get::<_, String>(0)?, "flags": row.get::<_, Option<String>>(1)?, "access": row.get::<_, Option<String>>(2)?, "detail": row.get::<_, Option<String>>(3)?, "return_type": row.get::<_, Option<String>>(4)?, "is_static": row.get::<_, i64>(5)? })))?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetClassMembersRecursive { class_name, namespace } => class::get_class_members_recursive(conn, class_name, namespace),
        QueryRequest::SearchFilesByPathPart { part } => asset::search_files_by_path_part(conn, part),
        QueryRequest::GetEnumValues { enum_name } => {
             let mut stmt = conn.prepare("SELECT sen.text FROM enum_values ev JOIN strings sen ON ev.name_id = sen.id JOIN classes c ON ev.enum_id = c.id JOIN strings sc ON c.name_id = sc.id WHERE sc.text = ? AND c.symbol_type = 'enum'")?;
             let rows = stmt.query_map([enum_name], |row| Ok(json!(row.get::<_, String>(0)?)))?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetComponents => module::get_components(conn),
        QueryRequest::GetModules => module::get_modules(conn),
        QueryRequest::GetModuleIdByName { name } => {
             let mut stmt = conn.prepare("SELECT m.id FROM modules m JOIN strings s ON m.name_id = s.id WHERE s.text = ?")?;
             if let Some(res) = stmt.query_row([name], |row| Ok(row.get::<_, i64>(0)?)).optional()? { return Ok(json!(res)); }
             Ok(Value::Null)
        },
        QueryRequest::GetModuleRootPath { name } => {
             let mut stmt = conn.prepare("SELECT sr.text FROM modules m JOIN strings sm ON m.name_id = sm.id JOIN strings sr ON m.root_path_id = sr.id WHERE sm.text = ?")?;
             if let Some(res) = stmt.query_row([name], |row| Ok(row.get::<_, String>(0)?)).optional()? { return Ok(json!(res)); }
             Ok(Value::Null)
        },
        QueryRequest::GetFilesInModule { module_id } => {
             let mut stmt = conn.prepare("SELECT s.text FROM files f JOIN strings s ON f.path_id = s.id WHERE f.module_id = ?")?;
             let rows = stmt.query_map([module_id], |row| Ok(json!(row.get::<_, String>(0)?)))?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetFilesInModules { modules, extensions, filter } => {
             if modules.is_empty() { return Ok(json!([])); }
             let mut all_files = Vec::new();
             for chunk in modules.chunks(500) {
                 let placeholders: Vec<String> = chunk.iter().map(|_| "?".to_string()).collect();
                 let mut sql = format!(
                    "SELECT sp.text as path, f.extension, sm.text as name, sr.text as root_path 
                     FROM files f 
                     JOIN strings sp ON f.path_id = sp.id
                     JOIN modules m ON f.module_id = m.id 
                     JOIN strings sm ON m.name_id = sm.id
                     JOIN strings sr ON m.root_path_id = sr.id
                     WHERE sm.text IN ({})", 
                    placeholders.join(",")
                 );
                 if let Some(exts) = &extensions { if !exts.is_empty() { let ext_placeholders: Vec<String> = exts.iter().map(|_| "?".to_string()).collect(); sql.push_str(&format!(" AND f.extension IN ({})", ext_placeholders.join(","))); } }
                 if filter.is_some() { sql.push_str(" AND sp.text LIKE ?"); }
                 let mut stmt = conn.prepare(&sql)?;
                 let mut params: Vec<&dyn ToSql> = chunk.iter().map(|s| s as &dyn ToSql).collect();
                 if let Some(exts) = &extensions { for ext in exts { params.push(ext); } }
                 if let Some(f) = &filter { params.push(f); }
                 let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| Ok(json!({ "file_path": row.get::<_, String>(0)?, "extension": row.get::<_, String>(1)?, "module_name": row.get::<_, String>(2)?, "module_root": row.get::<_, String>(3)? })))?;
                 for r in rows { all_files.push(r?); }
             }
             Ok(json!(all_files))
        },
        QueryRequest::SearchFilesInModules { modules, filter, limit } => asset::search_files_in_modules(conn, modules, filter, limit),
        QueryRequest::SearchSymbolsInModules { modules, symbol_type, filter, limit } => class::search_symbols_in_modules(conn, modules, symbol_type, filter, limit),
        QueryRequest::GetDirectoriesInModule { .. } => Ok(json!([])),
        QueryRequest::GetModuleFilesByNameAndRoot { name, root } => module::get_module_files_by_name_and_root(conn, name, root),
        QueryRequest::GetModuleDirsByNameAndRoot { .. } => Ok(json!([])),
        QueryRequest::GetClassFilePath { class_name } => {
             let mut stmt = conn.prepare("SELECT s.text FROM files f JOIN strings s ON f.path_id = s.id JOIN classes c ON c.file_id = f.id JOIN strings sc ON c.name_id = sc.id WHERE sc.text = ? LIMIT 1")?;
             let res = stmt.query_row([class_name], |row| Ok(row.get::<_, String>(0)?)).optional()?;
             Ok(json!(res))
        },
        QueryRequest::GetFileSymbols { file_path } => class::get_file_symbols(conn, file_path),
        QueryRequest::ParseBuffer { content, file_path, line, character } => buffer::parse_buffer(content, file_path, line, character),
        QueryRequest::UpdateMemberReturnType { .. } => {
             // String Interning対応の実装を保留
             Err(anyhow::anyhow!("UpdateMemberReturnType is not yet updated for String Interning"))
        },
        QueryRequest::GetTargetFiles => {
             let mut stmt = conn.prepare("SELECT s.text as path, sn.text as filename FROM files f JOIN strings s ON f.path_id = s.id JOIN strings sn ON f.filename_id = sn.id WHERE sn.text LIKE '%.Target.cs'")?;
             let rows = stmt.query_map([], |row| Ok(json!({ "path": row.get::<_, String>(0)?, "filename": row.get::<_, String>(1)? })))?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetAllFilePaths => {
             let mut stmt = conn.prepare("SELECT s.text FROM files f JOIN strings s ON f.path_id = s.id")?;
             let rows = stmt.query_map([], |row| Ok(json!(row.get::<_, String>(0)?)))?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GetAllFilesMetadata => {
             let mut stmt = conn.prepare("SELECT sn.text as filename, sp.text as path, sm.text as name FROM files f JOIN strings sn ON f.filename_id = sn.id JOIN strings sp ON f.path_id = sp.id JOIN modules m ON f.module_id = m.id JOIN strings sm ON m.name_id = sm.id")?;
             let rows = stmt.query_map([], |row| Ok(json!({ "filename": row.get::<_, String>(0)?, "path": row.get::<_, String>(1)?, "module_name": row.get::<_, String>(2)? })))?;
             Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
        },
        QueryRequest::GrepAssets { .. } |
        QueryRequest::GetFilesInModulesAsync { .. } | 
        QueryRequest::SearchFilesInModulesAsync { .. } |
        QueryRequest::GetClassesInModulesAsync { .. } => Err(anyhow::anyhow!("Async queries must be processed via process_query_streaming")),
        QueryRequest::GetCompletions { content, line, character, file_path } => crate::completion::process_completion(conn, &content, line, character, file_path),
    }
}