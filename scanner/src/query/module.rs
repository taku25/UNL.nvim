use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

pub fn load_component_data(conn: &Connection, component: String) -> anyhow::Result<Value> {
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

     let mut file_stmt = conn.prepare("SELECT f.id, f.path, f.filename, f.extension, f.is_header, f.module_id FROM files f WHERE f.module_id = ?")?;
     let mut class_stmt = conn.prepare("SELECT c.name, c.base_class, c.line_number FROM classes c WHERE c.file_id = ?")?;

     for mod_res in modules_iter {
         let (mid, mname, mtype, mroot, mpath) = mod_res?;
         let mut mod_data = json!({ "name": mname, "module_root": mroot, "path": mpath, "files": { "source": [], "config": [], "shader": [], "other": [] }, "header_details": {} });
         let files_iter = file_stmt.query_map([mid], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(3)?, row.get::<_, i64>(4)?)))?;
         for file_res in files_iter {
             let (fid, fpath, fext, is_header) = file_res?;
             let ext = fext.to_lowercase();
             if ["cpp", "c", "cc", "h", "hpp"].contains(&ext.as_str()) {
                 mod_data["files"]["source"].as_array_mut().unwrap().push(json!(fpath));
                 if is_header == 1 {
                     let classes_iter = class_stmt.query_map([fid], |row| Ok(json!({ "name": row.get::<_, String>(0)?, "base_class": row.get::<_, Option<String>>(1)?, "line_number": row.get::<_, i64>(2)? })))?;
                     let classes: Vec<Value> = classes_iter.collect::<Result<_, _>>()?;
                     if !classes.is_empty() { mod_data["header_details"].as_object_mut().unwrap().insert(fpath, json!({ "classes": classes })); }
                 }
             } else if ext == "ini" { mod_data["files"]["config"].as_array_mut().unwrap().push(json!(fpath)); }
             else if ext == "usf" || ext == "ush" { mod_data["files"]["shader"].as_array_mut().unwrap().push(json!(fpath)); }
             else { mod_data["files"]["other"].as_array_mut().unwrap().push(json!(fpath)); }
         }
         let target_key = match mtype.as_str() { "Runtime" => "runtime_modules", "Editor" => "editor_modules", "Developer" => "developer_modules", "Program" => "programs_modules", _ => "runtime_modules" };
         result[target_key].as_object_mut().unwrap().insert(mname, mod_data);
     }
     Ok(result)
}

pub fn get_module_by_name(conn: &Connection, name: String) -> anyhow::Result<Value> {
    let mut stmt = conn.prepare("SELECT m.id, m.name, m.type, m.scope, m.root_path, m.build_cs_path FROM modules m WHERE m.name = ? LIMIT 1")?;
    let mod_row = stmt.query_row([name], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(4)?, row.get::<_, Option<String>>(5)?))).optional()?;
    if let Some((mid, mname, mroot, mpath)) = mod_row {
        let mut mod_data = json!({ "name": mname, "module_root": mroot, "path": mpath, "files": { "source": [], "config": [], "shader": [], "other": [] } });
        let mut stmt = conn.prepare("SELECT f.path, f.extension FROM files f WHERE f.module_id = ?")?;
        let files_iter = stmt.query_map([mid], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
        for res in files_iter {
            let (fpath, fext) = res?;
            let ext = fext.to_lowercase();
            if ["cpp", "c", "cc", "h", "hpp"].contains(&ext.as_str()) { mod_data["files"]["source"].as_array_mut().unwrap().push(json!(fpath)); }
            else if ext == "ini" { mod_data["files"]["config"].as_array_mut().unwrap().push(json!(fpath)); }
            else if ext == "usf" || ext == "ush" { mod_data["files"]["shader"].as_array_mut().unwrap().push(json!(fpath)); }
            else { mod_data["files"]["other"].as_array_mut().unwrap().push(json!(fpath)); }
        }
        Ok(mod_data)
    } else { Ok(Value::Null) }
}

pub fn get_components(conn: &Connection) -> anyhow::Result<Value> {
     let mut stmt = conn.prepare("SELECT * FROM components ORDER BY name ASC")?;
     let rows = stmt.query_map([], |row| Ok(json!({ "id": row.get::<_, i64>("id")?, "name": row.get::<_, String>("name")?, "display_name": row.get::<_, Option<String>>("display_name")?, "type": row.get::<_, Option<String>>("type")?, "owner_name": row.get::<_, Option<String>>("owner_name")?, "root_path": row.get::<_, Option<String>>("root_path")?, "uplugin_path": row.get::<_, Option<String>>("uplugin_path")?, "uproject_path": row.get::<_, Option<String>>("uproject_path")?, "engine_association": row.get::<_, Option<String>>("engine_association")? })))?;
     Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
}

pub fn get_modules(conn: &Connection) -> anyhow::Result<Value> {
     let mut stmt = conn.prepare("SELECT * FROM modules ORDER BY name ASC")?;
     let rows = stmt.query_map([], |row| Ok(json!({ "id": row.get::<_, i64>("id")?, "name": row.get::<_, String>("name")?, "type": row.get::<_, Option<String>>("type")?, "scope": row.get::<_, Option<String>>("scope")?, "root_path": row.get::<_, String>("root_path")?, "build_cs_path": row.get::<_, Option<String>>("build_cs_path")?, "owner_name": row.get::<_, Option<String>>("owner_name")?, "component_name": row.get::<_, Option<String>>("component_name")?, "deep_dependencies": row.get::<_, Option<String>>("deep_dependencies")? })))?;
     Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
}

pub fn get_module_files_by_name_and_root(conn: &Connection, name: String, root: String) -> anyhow::Result<Value> {
     let mut stmt = conn.prepare("SELECT f.path, f.extension FROM files f JOIN modules m ON f.module_id = m.id WHERE m.name = ? AND m.root_path = ?")?;
     let rows = stmt.query_map([name, root], |row| Ok(json!({ "path": row.get::<_, String>(0)?, "extension": row.get::<_, String>(1)? })))?;
     Ok(json!(rows.collect::<Result<Vec<Value>, _>>()?))
}