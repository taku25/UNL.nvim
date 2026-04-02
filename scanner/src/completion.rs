use rusqlite::{Connection, params, OptionalExtension};
use serde_json::{json, Value};
use tree_sitter::{Parser, Point, Node, Query, QueryCursor, StreamingIterator};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crate::server::state::CompletionCache;

struct RequestContext<'a> {
    conn: &'a Connection,
    file_cache: HashMap<String, Vec<String>>,
    inheritance_cache: HashMap<(String, String), bool>,
    string_id_cache: HashMap<String, i64>,
}

impl<'a> RequestContext<'a> {
    fn new(conn: &'a Connection) -> Self {
        Self {
            conn,
            file_cache: HashMap::new(),
            inheritance_cache: HashMap::new(),
            string_id_cache: HashMap::new(),
        }
    }

    fn get_string_id(&mut self, text: &str) -> anyhow::Result<Option<i64>> {
        let text = text.trim();
        if text.is_empty() { return Ok(None); }
        if let Some(&id) = self.string_id_cache.get(text) {
            return Ok(Some(id));
        }
        let id: Option<i64> = self.conn.query_row(
            "SELECT id FROM strings WHERE text = ?",
            [text],
            |row| row.get(0)
        ).optional()?;
        if let Some(id_val) = id {
            self.string_id_cache.insert(text.to_string(), id_val);
        }
        Ok(id)
    }

    fn get_class_id_by_name(&mut self, class_name: &str) -> anyhow::Result<Option<i64>> {
        if let Some(name_id) = self.get_string_id(class_name)? {
            let class_id: Option<i64> = self.conn.query_row(
                "SELECT id FROM classes WHERE name_id = ? LIMIT 1",
                [name_id],
                |row| row.get(0)
            ).optional()?;
            return Ok(class_id);
        }
        Ok(None)
    }
}

// 補完ロジックのメインエントリー
pub fn process_completion(
    conn: &Connection,
    content: &str,
    line: u32,
    character: u32,
    _file_path: Option<String>,
    cache: Option<Arc<Mutex<CompletionCache>>>,
    persistent_cache: Option<Arc<Mutex<Connection>>>,
) -> anyhow::Result<Value> {
    tracing::info!("--- Completion Request at {}:{} ---", line, character);
    let mut ctx = RequestContext::new(conn);
    
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_unreal_cpp::LANGUAGE.into();
    parser.set_language(&language)?;

    let tree = parser.parse(content, None).ok_or_else(|| anyhow::anyhow!("Failed to parse content"))?;
    let root = tree.root_node();
    
    let row = line as usize;
    let col = character as usize;
    
    // カーソル位置とその直前を含むノードを探す (0.26.5 API)
    let point = Point::new(row, col);
    let prev_point = Point::new(row, if col > 0 { col - 1 } else { 0 });
    
    let node = match root.descendant_for_point_range(prev_point, point) {
        Some(n) => n,
        None => {
            tracing::info!("No node found at cursor position.");
            return Ok(json!([]));
        }
    };

    let node_type = node.kind();
    tracing::info!("Node at cursor: kind='{}', text='{}'", node_type, get_node_text(&node, content));
    
    // Check if we are inside or near an ERROR node
    let mut target_node = None;
    if node_type == "ERROR" || node_type == "." || node_type == "->" || node_type == "::" {
        if let Some(prev) = get_prev_meaningful_sibling(node) {
            tracing::info!("Found meaningful sibling before ERROR/Operator: kind='{}'", prev.kind());
            target_node = Some(prev);
        } else if let Some(parent) = node.parent() {
            if parent.kind() == "ERROR" {
                if let Some(prev) = get_prev_meaningful_sibling(parent) {
                    tracing::info!("Found meaningful sibling before parent ERROR: kind='{}'", prev.kind());
                    target_node = Some(prev);
                }
            }
        }
    }

    if let Some(t) = target_node {
        return resolve_node_and_fetch_members(&mut ctx, t, &root, content, row, None, cache, persistent_cache);
    }

    let mut curr_opt = Some(node);
    
    // 1. 演算子（. -> ::）の直後、または演算子そのものの場合
    if node_type == "." || node_type == "->" || node_type == "::" || node_type == ":" {
        let op_node = if node_type == ":" {
            node.parent().filter(|p| p.kind() == "::").unwrap_or(node)
        } else {
            node
        };

        if let Some(prev) = get_prev_meaningful_sibling(op_node) {
            tracing::info!("Operator detected (Case 1), target node: kind='{}', text='{}'", prev.kind(), get_node_text(&prev, content));
            return resolve_node_and_fetch_members(&mut ctx, prev, &root, content, row, None, cache, persistent_cache);
        } else {
            tracing::info!("Operator detected but no meaningful sibling found. Continuing to traverse up from parent.");
            curr_opt = node.parent(); // Move to parent and let Case 2 handle it
        }
    }

    // 2. 識別子の入力途中、またはそれ以外
    while let Some(curr) = curr_opt {
        let p_kind = curr.kind();
        tracing::debug!("Traversing up: kind='{}', text='{}'", p_kind, get_node_text(&curr, content));

        if p_kind == "unreal_macro_argument_list" || p_kind == "macro_argument_list" {
            if let Some(parent) = curr.parent() {
                let macro_name = get_node_text(&parent, content).trim();
                if let Some(res) = resolve_macro_specifiers(macro_name) {
                    tracing::info!("Resolved macro specifiers for '{}'", macro_name);
                    return Ok(res);
                }
                if let Some(grand) = parent.parent() {
                   let g_name = get_node_text(&grand, content).trim();
                   if let Some(res) = resolve_macro_specifiers(g_name) {
                       tracing::info!("Resolved macro specifiers for grand '{}'", g_name);
                       return Ok(res);
                   }
                }
            }
        }

        if p_kind == "field_expression" {
            let field_prefix = if let Some(field_node) = curr.child_by_field_name("field") {
                let text = get_node_text(&field_node, content);
                if text == "." || text == "->" { None } else { Some(text.to_string()) }
            } else { None };

            if let Some(obj_node) = curr.child_by_field_name("argument") {
                tracing::info!("Field expression detected (Case 2), resolving argument with prefix: {:?}", field_prefix);
                return resolve_node_and_fetch_members(&mut ctx, obj_node, &root, content, row, field_prefix, cache, persistent_cache);
            } else if let Some(first_child) = curr.child(0) {
                if first_child.kind() != "." && first_child.kind() != "->" {
                    tracing::info!("Field expression detected (Fallback), resolving first child...");
                    return resolve_node_and_fetch_members(&mut ctx, first_child, &root, content, row, field_prefix, cache, persistent_cache);
                }
            }
        } else if p_kind == "call_expression" && (node_type == "." || node_type == "->") {
             if let Some(func_node) = curr.child_by_field_name("function") {
                 tracing::info!("Call expression parent of operator detected, resolving function...");
                 return resolve_node_and_fetch_members(&mut ctx, func_node, &root, content, row, None, cache, persistent_cache);
             }
        } else if p_kind == "qualified_identifier" {
            let field_prefix = if let Some(name_node) = curr.child_by_field_name("name") {
                Some(get_node_text(&name_node, content).to_string())
            } else { None };

            if let Some(scope_node) = curr.child_by_field_name("scope") {
                tracing::info!("Qualified identifier detected (Case 2), resolving scope with prefix: {:?}", field_prefix);
                return resolve_static_members(&mut ctx, get_node_text(&scope_node, content), field_prefix, cache, persistent_cache);
            }
        } else if p_kind == "ERROR" {
            let count = curr.child_count();
            for i in (0..count).rev() {
                if let Some(child) = curr.child(i as u32) {
                    let ck = child.kind();
                    if ck == "." || ck == "->" || ck == "::" {
                        if let Some(prev) = get_prev_meaningful_sibling(child) {
                             tracing::info!("Operator detected inside ERROR, resolving previous sibling...");
                             return resolve_node_and_fetch_members(&mut ctx, prev, &root, content, row, None, cache, persistent_cache.clone());
                        }
                    }
                }
            }
        }
        curr_opt = curr.parent();
    }

    // 3. 暗黙の this 補完 + グローバルシンボル補完
    if node_type == "identifier" || node_type == "type_identifier" || node_type == "field_identifier" || node_type == "this" || node_type == "ERROR" {
        let prefix = get_node_text(&node, content).trim();
        let mut results = Vec::new();

        if let Some(current_class) = get_enclosing_class_name(&node, content) {
            if let Ok(members) = fetch_members_recursive(&mut ctx, &current_class, Some(prefix.to_string()), cache.as_ref().map(|c| Arc::clone(c)), persistent_cache.clone(), Some(&current_class)) {
                results.extend(members);
            }
        }

        if let Ok(globals) = fetch_global_symbols(conn, prefix) {
            if let Some(arr) = globals.as_array() {
                results.extend(arr.clone());
            }
        }

        results.extend(get_ue_snippets(prefix));

        if !results.is_empty() {
            let mut seen = HashMap::new();
            let mut unique_results = Vec::new();
            for r in results {
                if let Some(label) = r.get("label").and_then(|v| v.as_str()) {
                    if !seen.contains_key(label) {
                        seen.insert(label.to_string(), true);
                        unique_results.push(r);
                    }
                }
            }
            return Ok(json!(unique_results));
        }
    }

    Ok(json!([]))
}

fn get_node_text<'a>(node: &Node, content: &'a str) -> &'a str {
    let range = node.byte_range();
    if range.end <= content.len() {
        &content[range.start..range.end]
    } else {
        ""
    }
}

fn get_prev_meaningful_sibling(node: Node) -> Option<Node> {
    let mut curr = node.prev_sibling();
    while let Some(n) = curr {
        let kind = n.kind();
        if kind != "comment" && kind != " " && kind != "\n" && kind != "\r" {
            return Some(n);
        }
        curr = n.prev_sibling();
    }
    None
}

fn resolve_node_and_fetch_members(
    ctx: &mut RequestContext,
    node: Node,
    root: &Node,
    content: &str,
    cursor_row: usize,
    prefix: Option<String>,
    cache: Option<Arc<Mutex<CompletionCache>>>,
    persistent_cache: Option<Arc<Mutex<Connection>>>,
) -> anyhow::Result<Value> {
    if let Some(t_name) = resolve_expression_type(ctx, node, root, content, cursor_row)? {
        let resolved = resolve_typedef(ctx, &t_name)?;
        let current_class = get_enclosing_class_name(&node, content);
        tracing::info!("Final type for member lookup: '{}', current_class: {:?}, prefix: {:?}", resolved, current_class, prefix);
        
        let members = fetch_members_recursive(ctx, &resolved, prefix, cache, persistent_cache, current_class.as_deref())?;
        return Ok(json!(members));
    }
    Ok(json!([]))
}

fn resolve_expression_type(
    ctx: &mut RequestContext,
    node: Node,
    root: &Node,
    content: &str,
    cursor_row: usize,
) -> anyhow::Result<Option<String>> {
    let kind = node.kind();
    tracing::info!("resolve_expression_type(kind='{}', text='{}')", kind, get_node_text(&node, content));

    match kind {
        "this" => Ok(get_enclosing_class_name(&node, content)),
        "identifier" | "type_identifier" | "field_identifier" | "namespace_identifier" | "scoped_type_identifier" => {
            let name = get_node_text(&node, content).trim();
            if name.is_empty() { return Ok(None); }
            if name == "this" { return Ok(get_enclosing_class_name(&node, content)); }
            if let Some(t) = infer_variable_type(ctx, name, root, content, cursor_row)? {
                return Ok(Some(t));
            }
            if let Some(current_class) = get_enclosing_class_name(&node, content) {
                if let Some(rt) = find_member_return_type(ctx, &current_class, name)? {
                    return Ok(Some(rt));
                }
            }
            if is_known_type(ctx, name)? { return Ok(Some(name.to_string())); }
            Ok(None)
        }
        "qualified_identifier" => {
            let text = get_node_text(&node, content).trim();
            if is_known_type(ctx, text)? { return Ok(Some(text.to_string())); }
            if text.contains("::") {
                let parts: Vec<&str> = text.split("::").collect();
                if parts.len() >= 2 {
                    let cls = parts[..parts.len()-1].join("::");
                    let member = parts[parts.len()-1];
                    return find_member_return_type(ctx, &cls, member);
                }
            }
            Ok(None)
        }
        "template_call" | "template_function" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = get_node_text(&name_node, content).trim();
                if name == "Cast" || name == "StaticCast" || name == "ExactCast" || name == "CastChecked" {
                    if let Some(args_node) = node.child_by_field_name("arguments") {
                        let t_text = get_node_text(&args_node, content).trim();
                        if t_text.starts_with('<') && t_text.ends_with('>') {
                            let inner = &t_text[1..t_text.len()-1];
                            return Ok(Some(extract_clean_type(inner)));
                        }
                    }
                }
            }
            Ok(None)
        }
        "init_declarator" => {
            if let Some(val_node) = node.child_by_field_name("value") {
                return resolve_expression_type(ctx, val_node, root, content, cursor_row);
            }
            Ok(None)
        }
        "call_expression" => {
            if let Some(func_node) = node.child_by_field_name("function") {
                let func_kind = func_node.kind();
                if func_kind == "field_expression" {
                    if let Some(obj_node) = func_node.child_by_field_name("argument") {
                        if let Some(obj_type) = resolve_expression_type(ctx, obj_node, root, content, cursor_row)? {
                            if let Some(field_node) = func_node.child_by_field_name("field") {
                                return find_member_return_type(ctx, &obj_type, get_node_text(&field_node, content).trim());
                            }
                        }
                    }
                } else if func_kind == "template_call" || func_kind == "template_function" {
                    return resolve_expression_type(ctx, func_node, root, content, cursor_row);
                } else {
                    let func_name = get_node_text(&func_node, content).trim();
                    if func_name.contains("::") {
                        let parts: Vec<&str> = func_name.split("::").collect();
                        if parts.len() >= 2 {
                            let cls = parts[..parts.len()-1].join("::");
                            let method = parts[parts.len()-1];
                            return find_member_return_type(ctx, &cls, method);
                        }
                    }
                    if let Some(current_class) = get_enclosing_class_name(&node, content) {
                        return find_member_return_type(ctx, &current_class, func_name);
                    }
                }
            }
            Ok(None)
        }
        "field_expression" => {
            if let Some(obj_node) = node.child_by_field_name("argument") {
                if let Some(obj_type) = resolve_expression_type(ctx, obj_node, root, content, cursor_row)? {
                    if let Some(field_node) = node.child_by_field_name("field") {
                        return find_member_return_type(ctx, &obj_type, get_node_text(&field_node, content).trim());
                    }
                }
            }
            Ok(None)
        }
        "subscript_expression" => {
            if let Some(obj_node) = node.child_by_field_name("argument") {
                if let Some(obj_type) = resolve_expression_type(ctx, obj_node, root, content, cursor_row)? {
                    return Ok(Some(unwrap_container_type(&obj_type)));
                }
            }
            Ok(None)
        }
        "parenthesized_expression" | "pointer_expression" | "parenthesized_declarator" | "pointer_declarator" | "reference_declarator" => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i as u32) {
                    let ck = child.kind();
                    if ck != "(" && ck != ")" && ck != "*" && ck != "&" {
                        return resolve_expression_type(ctx, child, root, content, cursor_row);
                    }
                }
            }
            Ok(None)
        }
        _ => Ok(None)
    }
}

fn unwrap_container_type(t: &str) -> String {
    let t = t.trim();
    if let Some(start) = t.find('<') {
        if let Some(end) = t.rfind('>') {
            let wrapper = t[..start].trim();
            let inner = &t[start + 1..end];
            if wrapper == "TMap" {
                return get_template_argument(inner, 1).to_string();
            } else if wrapper == "TArray" || wrapper == "TSet" {
                return inner.to_string();
            }
        }
    }
    t.to_string()
}

fn get_template_argument(inner: &str, index: usize) -> &str {
    let mut depth = 0;
    let mut current_index = 0;
    let mut start = 0;
    for (i, c) in inner.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            ',' if depth == 0 => {
                if current_index == index { return inner[start..i].trim(); }
                start = i + 1;
                current_index += 1;
            }
            _ => {}
        }
    }
    if current_index == index { return inner[start..].trim(); }
    ""
}

fn find_member_return_type(ctx: &mut RequestContext, class_name: &str, member_name: &str) -> anyhow::Result<Option<String>> {
    let clean_class = extract_clean_type(class_name);
    let resolved_class = resolve_typedef(ctx, &clean_class)?;
    
    let start_class_id = match ctx.get_class_id_by_name(&resolved_class)? {
        Some(id) => id,
        None => return Ok(None),
    };

    let mut queue = vec![start_class_id];
    let mut visited = HashMap::new();
    while let Some(cls_id) = queue.pop() {
        if visited.contains_key(&cls_id) { continue; }
        visited.insert(cls_id, true);
        
        let mut stmt = ctx.conn.prepare("
            SELECT srt.text FROM members m 
            JOIN strings sm ON m.name_id = sm.id
            LEFT JOIN strings srt ON m.return_type_id = srt.id
            WHERE m.class_id = ? AND sm.text = ? 
            ORDER BY (CASE WHEN srt.text = 'T' OR srt.text = 'T*' OR srt.text = 'void' THEN 1 ELSE 0 END) ASC, length(srt.text) DESC 
            LIMIT 1
        ")?;
        let mut rows = stmt.query(params![cls_id, member_name])?;
        if let Some(row) = rows.next()? {
            if let Some(rt) = row.get::<_, Option<String>>(0)? {
                return Ok(Some(extract_clean_type(&rt)));
            }
        }
        
        let mut p_stmt = ctx.conn.prepare("
            SELECT parent_class_id, si.text FROM inheritance i 
            JOIN strings si ON i.parent_name_id = si.id
            WHERE child_id = ?")?;
        let p_rows = p_stmt.query_map([cls_id], |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, String>(1)?)))?;
        for p in p_rows { 
            let (p_id, p_name) = p?;
            if let Some(id) = p_id {
                queue.push(id);
            } else if let Some(id) = ctx.get_class_id_by_name(&p_name)? {
                queue.push(id);
            }
        }
    }
    Ok(None)
}

fn get_enclosing_class_name(start_node: &Node, content: &str) -> Option<String> {
    let mut curr_opt = Some(*start_node);
    while let Some(curr) = curr_opt {
        let kind = curr.kind();
        if kind == "class_specifier" || kind == "struct_specifier" || 
           kind == "unreal_class_declaration" || kind == "unreal_struct_declaration" {
            if let Some(name_node) = curr.child_by_field_name("name") {
                return Some(get_node_text(&name_node, content).trim().to_string());
            }
        } else if kind == "function_definition" {
            if let Some(decl) = curr.child_by_field_name("declarator") {
                if let Some(qualified) = find_qualified_identifier(decl) {
                    if let Some(scope) = qualified.child_by_field_name("scope") {
                        let text = get_node_text(&scope, content).trim().trim_end_matches("::");
                        return Some(extract_clean_type(text));
                    }
                }
            }
        }
        curr_opt = curr.parent();
    }
    None
}

fn find_qualified_identifier(node: Node) -> Option<Node> {
    if node.kind() == "qualified_identifier" { return Some(node); }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if let Some(res) = find_qualified_identifier(child) { return Some(res); }
        }
    }
    None
}

fn resolve_typedef(ctx: &mut RequestContext, type_name: &str) -> anyhow::Result<String> {
    let mut current = extract_clean_type(type_name);
    if current.is_empty() || current == "T" || current == "void" { return Ok(current); }
    for _ in 0..3 {
        let name_id = match ctx.get_string_id(&current)? {
            Some(id) => id,
            None => break,
        };

        let mut stmt = ctx.conn.prepare("
            SELECT sbc.text FROM classes c 
            JOIN strings sbc ON c.base_class_id = sbc.id
            WHERE c.name_id = ? AND symbol_type = 'typedef' LIMIT 1
        ")?;
        let mut rows = stmt.query([name_id])?;
        if let Some(row) = rows.next()? {
            if let Some(base) = row.get::<_, Option<String>>(0)? {
                let clean = extract_clean_type(&base);
                if clean == current || clean.is_empty() { break; }
                current = clean;
            } else { break; }
        } else { break; }
    }
    Ok(current)
}

fn resolve_static_members(
    ctx: &mut RequestContext,
    scope_name: &str,
    prefix: Option<String>,
    cache: Option<Arc<Mutex<CompletionCache>>>,
    persistent_cache: Option<Arc<Mutex<Connection>>>,
) -> anyhow::Result<Value> {
    let clean_scope = extract_clean_type(scope_name);
    let t_name = resolve_typedef(ctx, &clean_scope)?;
    let members = fetch_members_recursive(ctx, &t_name, prefix, cache, persistent_cache, None)?;
    Ok(json!(members))
}

fn is_subclass_of(ctx: &mut RequestContext, child: &str, parent: &str) -> anyhow::Result<bool> {
    if child == parent { return Ok(true); }
    let cache_key = (child.to_string(), parent.to_string());
    if let Some(&res) = ctx.inheritance_cache.get(&cache_key) { return Ok(res); }

    let child_id = match ctx.get_class_id_by_name(child)? {
        Some(id) => id,
        None => return Ok(false),
    };
    let parent_id = ctx.get_class_id_by_name(parent)?;

    let mut queue = vec![child_id];
    let mut visited = HashMap::new();
    let mut found = false;
    while let Some(current_id) = queue.pop() {
        if let Some(pid) = parent_id {
            if current_id == pid { found = true; break; }
        }
        
        if visited.contains_key(&current_id) { continue; }
        visited.insert(current_id, true);

        let mut stmt = ctx.conn.prepare("
            SELECT parent_class_id, si.text FROM inheritance i 
            JOIN strings si ON i.parent_name_id = si.id
            WHERE child_id = ?")?;
        let p_rows = stmt.query_map([current_id], |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, String>(1)?)))?;
        for p in p_rows {
            let (p_id, p_name) = p?;
            if p_name == parent { found = true; break; }
            if let Some(id) = p_id {
                queue.push(id);
            } else if let Some(id) = ctx.get_class_id_by_name(&p_name)? {
                queue.push(id);
            }
        }
        if found { break; }
    }
    ctx.inheritance_cache.insert(cache_key, found);
    Ok(found)
}

fn fetch_members_recursive(
    ctx: &mut RequestContext, 
    class_name: &str, 
    prefix: Option<String>, 
    cache: Option<Arc<Mutex<CompletionCache>>>,
    persistent_cache: Option<Arc<Mutex<Connection>>>,
    accessor_class: Option<&str>,
) -> anyhow::Result<Vec<Value>> {
    let prefix_val = prefix.as_deref().unwrap_or("");
    let accessor_val = accessor_class.unwrap_or("");
    let cache_key = format!("{}:{}:{}", class_name, prefix_val, accessor_val);
    
    // 1. Try Memory Cache
    if let Some(c_mutex) = &cache {
        let mut c = c_mutex.lock().unwrap();
        if let Some(cached) = c.get(&cache_key, "") {
            if let Some(arr) = cached.as_array() {
                return Ok(arr.clone());
            }
        }
    }

    // 2. Try Persistent Cache
    if let Some(pc_mutex) = &persistent_cache {
        let pc = pc_mutex.lock().unwrap();
        let mut stmt = pc.prepare("SELECT value FROM persistent_cache WHERE key = ?")?;
        let res: rusqlite::Result<Vec<u8>> = stmt.query_row([&cache_key], |row| row.get(0));
        if let Ok(blob) = res {
            if let Ok(arr_val) = serde_json::from_slice::<Value>(&blob) {
                if let Some(arr) = arr_val.as_array() {
                    return Ok(arr.clone());
                }
            }
        }
    }

    let start_class_id = match ctx.get_class_id_by_name(class_name)? {
        Some(id) => id,
        None => return Ok(Vec::new()),
    };

    let mut result = Vec::new();
    let mut queue = vec![start_class_id];
    let mut visited = HashMap::new();
    let prefix_search = prefix.as_ref().map(|p| format!("{}%", p));

    while let Some(current_class_id) = queue.pop() {
        if visited.contains_key(&current_class_id) { continue; }
        visited.insert(current_class_id, true);
        
        let mut sql = "
            SELECT smn.text, smt.text, srt.text, access, detail, m.line_number, sp.text
            FROM members m 
            JOIN strings smn ON m.name_id = smn.id
            JOIN strings smt ON m.type_id = smt.id
            LEFT JOIN strings srt ON m.return_type_id = srt.id
            LEFT JOIN files f ON m.file_id = f.id
            LEFT JOIN strings sp ON f.path_id = sp.id
            WHERE m.class_id = ?".to_string();
        
        if prefix_search.is_some() {
            sql.push_str(" AND smn.text LIKE ?");
        }
        sql.push_str(" ORDER BY smn.text ASC LIMIT 200");

        let mut mem_stmt = ctx.conn.prepare(&sql)?;
        
        type MemberRow = (String, String, Option<String>, Option<String>, Option<String>, usize, Option<String>);
        let member_data: Vec<MemberRow> = if let Some(p) = &prefix_search {
            let m_rows = mem_stmt.query_map(params![current_class_id, p], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<usize>>(5)?.unwrap_or(0),
                    row.get::<_, Option<String>>(6).ok().flatten(),
                ))
            })?;
            m_rows.filter_map(|r| r.ok()).collect()
        } else {
            let m_rows = mem_stmt.query_map([current_class_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<usize>>(5)?.unwrap_or(0),
                    row.get::<_, Option<String>>(6).ok().flatten(),
                ))
            })?;
            m_rows.filter_map(|r| r.ok()).collect()
        };

        for (m_name, m_type, r_type, access, detail, line, f_path) in member_data {
            let access_str = access.as_deref().unwrap_or("");

            // 判定を大幅に緩和
            let is_accessible = if accessor_val.is_empty() {
                // クラス外からのアクセス
                access_str == "public" || access_str.is_empty()
            } else if accessor_val == class_name {
                true
            } else if access_str == "private" {
                false
            } else if access_str == "protected" {
                is_subclass_of(ctx, accessor_val, class_name).unwrap_or(false)
            } else {
                true // public or empty
            };

            if !is_accessible { continue; }

            let doc = if let Some(path) = f_path {
                let mut comment = extract_comment_from_file(&path, line, &mut ctx.file_cache);
                if let Some(d) = &detail {
                    if !comment.is_empty() { comment.push_str("\n\n"); }
                    comment.push_str(d);
                }
                comment
            } else {
                detail.clone().unwrap_or_default()
            };

            result.push(json!({ 
                "label": m_name, 
                "kind": map_kind(&m_type), 
                "detail": r_type.unwrap_or_default(), 
                "documentation": doc, 
                "insertText": m_name 
            }));
        }

        let mut enum_stmt = ctx.conn.prepare("SELECT sen.text FROM enum_values ev JOIN strings sen ON ev.name_id = sen.id WHERE enum_id = ?")?;
        let enum_rows = enum_stmt.query_map([current_class_id], |row| {
            let e_name: String = row.get(0)?;
            Ok(json!({ "label": e_name, "kind": 20, "detail": "enum item", "insertText": e_name }))
        })?;
        for e in enum_rows { result.push(e?); }
        
        let mut parent_stmt = ctx.conn.prepare("SELECT parent_class_id, si.text FROM inheritance i JOIN strings si ON i.parent_name_id = si.id WHERE child_id = ?")?;
        let p_rows = parent_stmt.query_map([current_class_id], |row| {
            let p_id: Option<i64> = row.get(0)?;
            let p_name: String = row.get(1)?;
            Ok((p_id, p_name))
        })?;
        for p in p_rows {
            let (p_id, p_name) = p?;
            if let Some(id) = p_id {
                queue.push(id);
            } else {
                // IDがない場合は名前で検索（外部定義など）
                if let Some(id) = ctx.get_class_id_by_name(&p_name)? {
                    queue.push(id);
                }
            }
        }
        if result.len() >= 500 { break; }
    }

    let result_json = json!(result);

    // 3. Store in Memory Cache
    if let Some(c_mutex) = &cache {
        let mut c = c_mutex.lock().unwrap();
        c.put(&cache_key, "", result_json.clone());
    }

    // 4. Store in Persistent Cache
    if let Some(pc_mutex) = &persistent_cache {
        let pc = pc_mutex.lock().unwrap();
        if let Ok(blob) = serde_json::to_vec(&result_json) {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
            let _ = pc.execute("INSERT OR REPLACE INTO persistent_cache (key, value, last_used) VALUES (?, ?, ?)", params![cache_key, blob, now]);
        }
    }

    Ok(result)
}

fn fetch_global_symbols(conn: &Connection, prefix: &str) -> anyhow::Result<Value> {
    let mut results = Vec::new();
    let mut stmt = conn.prepare("SELECT s.text, symbol_type FROM classes c JOIN strings s ON c.name_id = s.id WHERE s.text LIKE ? AND symbol_type IN ('class', 'struct', 'enum') LIMIT 50")?;
    let rows = stmt.query_map([format!("{}%", prefix)], |row| {
        let name: String = row.get(0)?;
        let sym_type: String = row.get(1)?;
        let kind = match sym_type.as_str() { "class" | "struct" => 7, "enum" => 13, _ => 1 };
        Ok(json!({ "label": name, "kind": kind, "detail": sym_type, "insertText": name }))
    })?;
    for r in rows { results.push(r?); }
    Ok(json!(results))
}

fn get_ue_snippets(prefix: &str) -> Vec<Value> {
    let mut snippets = vec![
        json!({ "label": "UPROPERTY", "kind": 15, "detail": "macro", "insertText": "UPROPERTY($1)", "sortText": "01" }),
        json!({ "label": "UFUNCTION", "kind": 15, "detail": "macro", "insertText": "UFUNCTION($1)", "sortText": "01" }),
        json!({ "label": "UCLASS", "kind": 15, "detail": "macro", "insertText": "UCLASS($1)", "sortText": "01" }),
        json!({ "label": "USTRUCT", "kind": 15, "detail": "macro", "insertText": "USTRUCT($1)", "sortText": "01" }),
        json!({ "label": "UENUM", "kind": 15, "detail": "macro", "insertText": "UENUM($1)", "sortText": "01" }),
        json!({ "label": "GENERATED_BODY", "kind": 15, "detail": "macro", "insertText": "GENERATED_BODY()", "sortText": "01" }),
        json!({ "label": "GetWorld()", "kind": 2, "detail": "AActor", "insertText": "GetWorld()", "sortText": "02" }),
        json!({ "label": "GetOwner()", "kind": 2, "detail": "AActor", "insertText": "GetOwner()", "sortText": "02" }),
        json!({ "label": "Super::", "kind": 7, "detail": "parent class", "insertText": "Super::", "sortText": "00" }),
    ];
    if prefix.is_empty() { return snippets; }
    let prefix_lower = prefix.to_lowercase();
    snippets.retain(|v| v.get("label").and_then(|l| l.as_str()).map(|l| l.to_lowercase().starts_with(&prefix_lower)).unwrap_or(false));
    snippets
}

fn resolve_macro_specifiers(macro_name: &str) -> Option<Value> {
    let name = macro_name.split('(').next().unwrap_or("").trim();
    let mut items = Vec::new();
    match name {
        "UPROPERTY" => {
            items.push(json!({ "label": "EditAnywhere", "kind": 12, "detail": "Specifier", "documentation": "Indicates that this property can be edited by property windows, on instances or archetypes." }));
            items.push(json!({ "label": "BlueprintReadOnly", "kind": 12, "detail": "Specifier", "documentation": "This property can be read from blueprints, but never modified." }));
            items.push(json!({ "label": "BlueprintReadWrite", "kind": 12, "detail": "Specifier", "documentation": "This property can be read or written from blueprints." }));
            items.push(json!({ "label": "Category", "kind": 12, "detail": "Specifier", "documentation": "Specifies the category of the property in the Editor UI." }));
        },
        "UFUNCTION" => {
            items.push(json!({ "label": "BlueprintCallable", "kind": 12, "detail": "Specifier", "documentation": "The function can be executed in a Blueprint." }));
            items.push(json!({ "label": "BlueprintPure", "kind": 12, "detail": "Specifier", "documentation": "The function does not affect the owning object." }));
        },
        "UCLASS" | "USTRUCT" => {
            items.push(json!({ "label": "Blueprintable", "kind": 12, "detail": "Specifier" }));
            items.push(json!({ "label": "BlueprintType", "kind": 12, "detail": "Specifier" }));
        },
        _ => return None
    }
    if items.is_empty() { None } else { Some(json!(items)) }
}

fn map_kind(k: &str) -> i64 {
    match k { "function" => 2, "variable" | "property" => 5, "enum_item" => 20, _ => 1 }
}

fn extract_comment_from_file(file_path: &str, line_number: usize, file_cache: &mut HashMap<String, Vec<String>>) -> String {
    if line_number == 0 { return String::new(); }
    
    if !file_cache.contains_key(file_path) {
        if let Ok(content) = std::fs::read_to_string(file_path) {
            let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            file_cache.insert(file_path.to_string(), lines);
        } else {
            return String::new();
        }
    }
    
    let lines = file_cache.get(file_path).unwrap();
    if line_number > lines.len() { return String::new(); }
    let mut comment_lines = Vec::new();
    let mut current_line = line_number - 1;
    while current_line > 0 {
        let trimmed = lines[current_line - 1].trim();
        if trimmed.is_empty() || trimmed.starts_with('[') || trimmed.starts_with("UPROPERTY") || trimmed.starts_with("UFUNCTION") || trimmed.starts_with("GENERATED_BODY") {
            current_line -= 1; continue;
        }
        break;
    }
    let mut in_block_comment = false;
    while current_line > 0 {
        let trimmed = lines[current_line - 1].trim();
        if trimmed.starts_with("//") {
            comment_lines.push(trimmed.trim_start_matches('/').trim_start_matches('/').trim().to_string());
            current_line -= 1;
        } else if trimmed.ends_with("*/") {
            in_block_comment = true;
            let content = trimmed.trim_end_matches("*/");
            if content.starts_with("/*") { comment_lines.push(content.trim_start_matches("/*").trim_start_matches('*').trim().to_string()); break; }
            comment_lines.push(content.trim_start_matches('*').trim().to_string());
            current_line -= 1;
        } else if in_block_comment {
            if trimmed.starts_with("/*") { comment_lines.push(trimmed.trim_start_matches("/*").trim_start_matches('*').trim().to_string()); break; }
            comment_lines.push(trimmed.trim_start_matches('*').trim().to_string());
            current_line -= 1;
        } else { break; }
    }
    comment_lines.reverse();
    comment_lines.join("\n")
}

fn is_known_type(ctx: &mut RequestContext, name: &str) -> anyhow::Result<bool> {
    let clean = extract_clean_type(name);
    if clean.is_empty() { return Ok(false); }
    if let Some(id) = ctx.get_string_id(&clean)? {
        let exists: bool = ctx.conn.query_row(
            "SELECT 1 FROM classes WHERE name_id = ? LIMIT 1",
            [id],
            |_| Ok(true)
        ).optional()?.unwrap_or(false);
        return Ok(exists);
    }
    Ok(false)
}

fn infer_variable_type(ctx: &mut RequestContext, target_name: &str, root: &Node, content: &str, cursor_row: usize) -> anyhow::Result<Option<String>> {
    let language: tree_sitter::Language = tree_sitter_unreal_cpp::LANGUAGE.into();
    let query_str = "(declaration type: (_) @type declarator: (init_declarator declarator: (_) @decl)) (declaration type: (_) @type declarator: (_) @decl) (field_declaration type: (_) @type declarator: (init_declarator declarator: (_) @decl)) (field_declaration type: (_) @type declarator: (_) @decl) (parameter_declaration type: (_) @type declarator: (_) @decl) (for_range_loop type: (_) @type declarator: (_) @decl) (condition_clause value: (declaration type: (_) @type declarator: (init_declarator declarator: (_) @decl))) (condition_clause value: (declaration type: (_) @type declarator: (_) @decl)) (condition_clause (declaration type: (_) @type declarator: (init_declarator declarator: (_) @decl))) (field_declaration (_) @type (init_declarator declarator: (_) @decl)) (field_declaration (_) @type (_) @decl) (declaration (_) @type (init_declarator declarator: (_) @decl))";
    let query = Query::new(&language, query_str)?;
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, *root, content.as_bytes());
    let mut best_type = None;
    let mut best_row = 0;
    while let Some(m) = matches.next() {
        let mut type_node = None;
        let mut decl_nodes = Vec::new();
        for cap in m.captures {
            let c_name = query.capture_names()[cap.index as usize];
            if c_name == "type" { type_node = Some(cap.node); }
            else if c_name == "decl" { decl_nodes.push(cap.node); }
        }
        if let Some(t_node) = type_node {
            for d_node in decl_nodes {
                if find_identifier_in_decl(&d_node, target_name, content)? {
                    let row = d_node.start_position().row;
                    if row <= cursor_row && (best_type.is_none() || row >= best_row) {
                        let type_text = get_node_text(&t_node, content).trim();
                        if type_text == "auto" { if let Some(inferred) = infer_from_assignment(ctx, target_name, root, content, cursor_row)? { best_type = Some(inferred); } }
                        else { best_type = Some(extract_clean_type(type_text)); }
                        best_row = row;
                    }
                }
            }
        }
    }
    if best_type.is_none() { best_type = infer_from_assignment(ctx, target_name, root, content, cursor_row)?; }
    Ok(best_type)
}

fn find_identifier_in_decl(node: &Node, target_name: &str, content: &str) -> anyhow::Result<bool> {
    let kind = node.kind();
    if kind == "identifier" || kind == "field_identifier" { return Ok(get_node_text(node, content).trim() == target_name); }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if let Some(field_name) = node.field_name_for_child(i as u32) { if field_name == "value" || field_name == "initializer" { continue; } }
            if find_identifier_in_decl(&child, target_name, content)? { return Ok(true); }
        }
    }
    Ok(false)
}

fn infer_from_assignment(ctx: &mut RequestContext, target_name: &str, root: &Node, content: &str, cursor_row: usize) -> anyhow::Result<Option<String>> {
    let language: tree_sitter::Language = tree_sitter_unreal_cpp::LANGUAGE.into();
    let query_str = "(declaration type: (_) declarator: (init_declarator declarator: (_) @decl value: (_) @value)) (declaration declarator: (_) @decl value: (_) @value) (condition_clause value: (declaration type: (_) declarator: (init_declarator declarator: (_) @decl value: (_) @value))) (condition_clause value: (declaration type: (_) declarator: (_) @decl value: (_) @value)) (condition_clause value: (declaration declarator: (_) @decl value: (_) @value)) (assignment_expression left: (_) @decl right: (_) @value)";
    let query = Query::new(&language, query_str)?;
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, *root, content.as_bytes());
    while let Some(m) = matches.next() {
        let (mut decl_node, mut value_node) = (None, None);
        for cap in m.captures {
            let c_name = query.capture_names()[cap.index as usize];
            if c_name == "decl" { decl_node = Some(cap.node); } else if c_name == "value" { value_node = Some(cap.node); }
        }
        if let (Some(d_node), Some(v_node)) = (decl_node, value_node) {
            let dk = d_node.kind();
            if dk == "subscript_expression" || dk == "field_expression" { continue; }
            let found_name = if dk == "identifier" { get_node_text(&d_node, content).trim() } else { "" };
            if !found_name.is_empty() && found_name == target_name {
                let row = d_node.start_position().row;
                if row <= cursor_row { 
                    if let Ok(Some(t)) = resolve_expression_type(ctx, v_node, root, content, cursor_row) { return Ok(Some(t)); }
                    return infer_from_value_text(get_node_text(&v_node, content));
                }
            } else if find_identifier_in_decl(&d_node, target_name, content)? {
                let row = d_node.start_position().row;
                if row <= cursor_row { if let Ok(Some(t)) = resolve_expression_type(ctx, v_node, root, content, cursor_row) { return Ok(Some(t)); } }
            }
        }
    }
    Ok(None)
}

fn infer_from_value_text(text: &str) -> anyhow::Result<Option<String>> {
    let text = text.trim();
    if let Ok(re) = regex::Regex::new(r"CreateDefaultSubobject\s*<\s*([a-zA-Z0-9_:]+)") { if let Some(cap) = re.captures(text) { return Ok(Some(extract_clean_type(cap.get(1).unwrap().as_str()))); } }
    if let Ok(re) = regex::Regex::new(r"([a-zA-Z0-9_]+)\s*<\s*([a-zA-Z0-9_:]+)") {
        if let Some(cap) = re.captures(text) {
            let func = cap.get(1).unwrap().as_str(); let inner = cap.get(2).unwrap().as_str();
            if ["NewObject", "TObjectPtr", "TSharedPtr"].contains(&func) { return Ok(Some(extract_clean_type(inner))); }
            return Ok(Some(extract_clean_type(func)));
        }
    }
    if let Ok(re) = regex::Regex::new(r"^([a-zA-Z0-9_:]+)\s*\(") { if let Some(cap) = re.captures(text) { return Ok(Some(extract_clean_type(cap.get(1).unwrap().as_str()))); } }
    Ok(None)
}

fn extract_clean_type(raw: &str) -> String {
    let mut clean = raw.trim().to_string();
    let keywords = ["const", "typename", "struct", "class", "enum", "virtual", "static", "inline", "FORCEINLINE"];
    for kw in keywords { if let Ok(re) = regex::Regex::new(&format!(r"\b{}\b", kw)) { clean = re.replace_all(&clean, "").to_string(); } }
    if let Ok(re) = regex::Regex::new(r"\b[A-Z0-9_]+_API\b") { clean = re.replace_all(&clean, "").to_string(); }
    clean = clean.trim().to_string();
    if let Some(start) = clean.find('<') {
        if let Some(end) = clean.rfind('>') {
            let wrapper = clean[..start].trim(); let inner = &clean[start+1..end];
            if ["TObjectPtr", "TSharedPtr", "TUniquePtr", "TWeakObjectPtr", "TSubclassOf", "TSoftObjectPtr", "TSoftClassPtr", "TEnumAsByte"].contains(&wrapper) { return extract_clean_type(inner); }
            return format!("{}<{}>{}", wrapper.replace('*', "").replace('&', "").trim(), inner, clean[end+1..].replace('*', "").replace('&', "").trim()).trim().to_string();
        }
    }
    clean.replace('*', " ").replace('&', " ").split_whitespace().last().unwrap_or("").split("::").last().unwrap_or("").to_string()
}
