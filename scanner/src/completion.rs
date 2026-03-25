use rusqlite::Connection;
use serde_json::{json, Value};
use tree_sitter::{Parser, Point, Node, Query, QueryCursor, StreamingIterator};
use std::collections::HashMap;

// 補完ロジックのメインエントリー
pub fn process_completion(
    conn: &Connection,
    content: &str,
    line: u32,
    character: u32,
    _file_path: Option<String>,
) -> anyhow::Result<Value> {
    tracing::info!("--- Completion Request at {}:{} ---", line, character);
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
        return resolve_node_and_fetch_members(conn, t, &root, content, row);
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
            return resolve_node_and_fetch_members(conn, prev, &root, content, row);
        } else {
            tracing::info!("Operator detected but no meaningful sibling found. Continuing to traverse up from parent.");
            curr_opt = node.parent(); // Move to parent and let Case 2 handle it
        }
    }

    // 2. 識別子の入力途中、またはそれ以外
    while let Some(curr) = curr_opt {
        let p_kind = curr.kind();
        tracing::debug!("Traversing up: kind='{}', text='{}'", p_kind, get_node_text(&curr, content));

        // ... existing macro specifier logic ...
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
            if let Some(obj_node) = curr.child_by_field_name("argument") {
                tracing::info!("Field expression detected (Case 2), resolving argument...");
                return resolve_node_and_fetch_members(conn, obj_node, &root, content, row);
            } else if let Some(first_child) = curr.child(0) {
                // Fallback: use first child if argument field is missing but expression is present
                if first_child.kind() != "." && first_child.kind() != "->" {
                    tracing::info!("Field expression detected (Fallback), resolving first child...");
                    return resolve_node_and_fetch_members(conn, first_child, &root, content, row);
                }
            }
        } else if p_kind == "call_expression" && (node_type == "." || node_type == "->") {
             // Sometimes the dot is a child of call_expression if it's incomplete
             if let Some(func_node) = curr.child_by_field_name("function") {
                 tracing::info!("Call expression parent of operator detected, resolving function...");
                 return resolve_node_and_fetch_members(conn, func_node, &root, content, row);
             }
        } else if p_kind == "qualified_identifier" {
            if let Some(scope_node) = curr.child_by_field_name("scope") {
                tracing::info!("Qualified identifier detected (Case 2), resolving scope...");
                return resolve_static_members(conn, get_node_text(&scope_node, content));
            }
        } else if p_kind == "ERROR" {
            let count = curr.child_count();
            for i in (0..count).rev() {
                if let Some(child) = curr.child(i as u32) {
                    let ck = child.kind();
                    if ck == "." || ck == "->" || ck == "::" {
                        if let Some(prev) = get_prev_meaningful_sibling(child) {
                             tracing::info!("Operator detected inside ERROR, resolving previous sibling...");
                             return resolve_node_and_fetch_members(conn, prev, &root, content, row);
                        }
                    }
                }
            }
        }
        curr_opt = curr.parent();
    }

    // 3. 暗黙の this 補完 + グローバルシンボル補完 (スタンドアロンの識別子入力時)
    if node_type == "identifier" || node_type == "type_identifier" || node_type == "field_identifier" || node_type == "this" || node_type == "ERROR" {
        let prefix = get_node_text(&node, content).trim();
        let mut results = Vec::new();

        // 3a. Current class members (implicit this)
        if let Some(current_class) = get_enclosing_class_name(&node, content) {
            tracing::info!("Implicit 'this' context detected: '{}'", current_class);
            if let Ok(members) = fetch_members_recursive(conn, &current_class) {
                results.extend(members);
            }
        }

        // 3b. Global symbols (Classes, Enums)
        if let Ok(globals) = fetch_global_symbols(conn, prefix) {
            if let Some(arr) = globals.as_array() {
                results.extend(arr.clone());
            }
        }

        // 3c. Unreal Snippets
        results.extend(get_ue_snippets(prefix));

        if !results.is_empty() {
            // 重複排除 (labelで)
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
    conn: &Connection,
    node: Node,
    root: &Node,
    content: &str,
    cursor_row: usize,
) -> anyhow::Result<Value> {
    if let Some(t_name) = resolve_expression_type(conn, node, root, content, cursor_row)? {
        let resolved = resolve_typedef(conn, &t_name)?;
        tracing::info!("Final type for member lookup: '{}'", resolved);
        
        let members = fetch_members_recursive(conn, &resolved)?;
        return Ok(json!(members));
    }
    Ok(json!([]))
}

fn resolve_expression_type(
    conn: &Connection,
    node: Node,
    root: &Node,
    content: &str,
    cursor_row: usize,
) -> anyhow::Result<Option<String>> {
    let kind = node.kind();
    tracing::info!("resolve_expression_type(kind='{}', text='{}')", kind, get_node_text(&node, content));

    match kind {
        "this" => {
            let cls = get_enclosing_class_name(&node, content);
            tracing::info!("Resolved 'this' to class: {:?}", cls);
            Ok(cls)
        }
        "identifier" | "type_identifier" | "field_identifier" | "namespace_identifier" | "scoped_type_identifier" => {
            let name = get_node_text(&node, content).trim();
            if name.is_empty() { return Ok(None); }
            if name == "this" {
                return Ok(get_enclosing_class_name(&node, content));
            }
            if let Some(t) = infer_variable_type(conn, name, root, content, cursor_row)? {
                return Ok(Some(t));
            }
            if let Some(current_class) = get_enclosing_class_name(&node, content) {
                tracing::info!("Checking if '{}' is a member variable of '{}'", name, current_class);
                if let Some(rt) = find_member_return_type(conn, &current_class, name)? {
                    return Ok(Some(rt));
                }
            }
            
            // Fallback: Check if it's a known type (Class or Enum)
            if is_known_type(conn, name)? {
                return Ok(Some(name.to_string()));
            }

            Ok(None)
        }
        "qualified_identifier" => {
            let text = get_node_text(&node, content).trim();
            if is_known_type(conn, text)? {
                return Ok(Some(text.to_string()));
            }
            // If it's something like Class::StaticMember, try to resolve the type
            if text.contains("::") {
                let parts: Vec<&str> = text.split("::").collect();
                if parts.len() >= 2 {
                    let cls = parts[..parts.len()-1].join("::");
                    let member = parts[parts.len()-1];
                    return find_member_return_type(conn, &cls, member);
                }
            }
            Ok(None)
        }
        "template_call" => {
            // Support Cast<T>(Obj) or StaticCast<T>(Obj)
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = get_node_text(&name_node, content).trim();
                if name == "Cast" || name == "StaticCast" || name == "ExactCast" || name == "CastChecked" {
                    if let Some(args_node) = node.child_by_field_name("arguments") {
                        // arguments are template arguments <T>
                        let t_text = get_node_text(&args_node, content).trim();
                        if t_text.starts_with('<') && t_text.ends_with('>') {
                            let inner = &t_text[1..t_text.len()-1];
                            let clean = extract_clean_type(inner);
                            tracing::info!("Resolved template cast '{}' to type '{}'", name, clean);
                            return Ok(Some(clean));
                        }
                    }
                }
            }
            Ok(None)
        }
        "init_declarator" => {
            if let Some(val_node) = node.child_by_field_name("value") {
                return resolve_expression_type(conn, val_node, root, content, cursor_row);
            }
            Ok(None)
        }
        "call_expression" => {
            if let Some(func_node) = node.child_by_field_name("function") {
                let func_kind = func_node.kind();
                if func_kind == "field_expression" {
                    if let Some(obj_node) = func_node.child_by_field_name("argument") {
                        if let Some(obj_type) = resolve_expression_type(conn, obj_node, root, content, cursor_row)? {
                            if let Some(field_node) = func_node.child_by_field_name("field") {
                                return find_member_return_type(conn, &obj_type, get_node_text(&field_node, content).trim());
                            }
                        }
                    }
                } else if func_kind == "template_call" {
                    return resolve_expression_type(conn, func_node, root, content, cursor_row);
                } else {
                    let func_name = get_node_text(&func_node, content).trim();
                    // --- Added: Handle Class::Get() ---
                    if func_name.contains("::") {
                        let parts: Vec<&str> = func_name.split("::").collect();
                        if parts.len() >= 2 {
                            let cls = parts[..parts.len()-1].join("::");
                            let method = parts[parts.len()-1];
                            return find_member_return_type(conn, &cls, method);
                        }
                    }
                    // ----------------------------------
                    if let Some(current_class) = get_enclosing_class_name(&node, content) {
                        return find_member_return_type(conn, &current_class, func_name);
                    }
                }
            }
            Ok(None)
        }
        "field_expression" => {
            if let Some(obj_node) = node.child_by_field_name("argument") {
                if let Some(obj_type) = resolve_expression_type(conn, obj_node, root, content, cursor_row)? {
                    if let Some(field_node) = node.child_by_field_name("field") {
                        return find_member_return_type(conn, &obj_type, get_node_text(&field_node, content).trim());
                    }
                }
            }
            Ok(None)
        }
        "subscript_expression" => {
            if let Some(obj_node) = node.child_by_field_name("argument") {
                if let Some(obj_type) = resolve_expression_type(conn, obj_node, root, content, cursor_row)? {
                    let unwrapped = unwrap_container_type(&obj_type);
                    tracing::info!("Subscript detected: container type '{}' -> element type '{}'", obj_type, unwrapped);
                    return Ok(Some(unwrapped));
                }
            }
            Ok(None)
        }
        "parenthesized_expression" => {
            // Skip parentheses ( (expr) )
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i as u32) {
                    let ck = child.kind();
                    if ck != "(" && ck != ")" {
                        return resolve_expression_type(conn, child, root, content, cursor_row);
                    }
                }
            }
            Ok(None)
        }
        "pointer_expression" | "parenthesized_declarator" | "pointer_declarator" | "reference_declarator" => {
            // Unwrap pointer/reference expressions
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i as u32) {
                    let ck = child.kind();
                    if ck != "*" && ck != "&" {
                        return resolve_expression_type(conn, child, root, content, cursor_row);
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

fn find_member_return_type(conn: &Connection, class_name: &str, member_name: &str) -> anyhow::Result<Option<String>> {
    let clean_class = extract_clean_type(class_name);
    let resolved_class = resolve_typedef(conn, &clean_class)?;
    tracing::info!("Searching member '{}' in class '{}' (and parents)", member_name, resolved_class);
    
    let mut queue = vec![resolved_class];
    let mut visited = HashMap::new();
    while let Some(cls) = queue.pop() {
        if visited.contains_key(&cls) { continue; }
        visited.insert(cls.clone(), true);
        
        let mut stmt = conn.prepare("
            SELECT srt.text FROM members m 
            JOIN classes c ON m.class_id = c.id 
            JOIN strings sc ON c.name_id = sc.id
            JOIN strings sm ON m.name_id = sm.id
            LEFT JOIN strings srt ON m.return_type_id = srt.id
            WHERE sc.text = ? AND sm.text = ? 
            ORDER BY (CASE WHEN srt.text = 'T' OR srt.text = 'T*' OR srt.text = 'void' THEN 1 ELSE 0 END) ASC, length(srt.text) DESC 
            LIMIT 1
        ")?;
        let mut rows = stmt.query([&cls, member_name])?;
        if let Some(row) = rows.next()? {
            if let Some(rt) = row.get::<_, Option<String>>(0)? {
                let cleaned = extract_clean_type(&rt);
                tracing::info!("Found member '{}' -> '{}' in '{}'", member_name, cleaned, cls);
                return Ok(Some(cleaned));
            }
        }
        
        let mut p_stmt = conn.prepare("
            SELECT si.text FROM inheritance i 
            JOIN classes c ON i.child_id = c.id 
            JOIN strings sc ON c.name_id = sc.id
            JOIN strings si ON i.parent_name_id = si.id
            WHERE sc.text = ?")?;
        let p_rows = p_stmt.query_map([&cls], |r| Ok(r.get::<_, String>(0)?))?;
        for p in p_rows { queue.push(p?); }
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
                let name = get_node_text(&name_node, content).trim().to_string();
                tracing::info!("Enclosing class found via specifier: '{}'", name);
                return Some(name);
            }
        } else if kind == "function_definition" {
            if let Some(decl) = curr.child_by_field_name("declarator") {
                if let Some(qualified) = find_qualified_identifier(decl) {
                    if let Some(scope) = qualified.child_by_field_name("scope") {
                        let text = get_node_text(&scope, content).trim().trim_end_matches("::");
                        let clean = extract_clean_type(text);
                        tracing::info!("Enclosing class found via qualified method: '{}'", clean);
                        return Some(clean);
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

fn resolve_typedef(conn: &Connection, type_name: &str) -> anyhow::Result<String> {
    let mut current = extract_clean_type(type_name);
    if current.is_empty() || current == "T" || current == "void" { return Ok(current); }
    for _ in 0..3 {
        // String Interning support for base_class (need to join strings if it was stored as ID, but here it might be raw text still or ID?)
        // Let's assume symbol_type 'typedef' still stores base_class as a text ID or raw text.
        // Actually classes table has base_class_id now.
        let mut stmt = conn.prepare("
            SELECT sbc.text FROM classes c 
            JOIN strings sc ON c.name_id = sc.id
            JOIN strings sbc ON c.base_class_id = sbc.id
            WHERE sc.text = ? AND symbol_type = 'typedef' LIMIT 1
        ")?;
        let mut rows = stmt.query([&current])?;
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

fn resolve_static_members(conn: &Connection, scope_name: &str) -> anyhow::Result<Value> {
    let clean_scope = extract_clean_type(scope_name);
    let t_name = resolve_typedef(conn, &clean_scope)?;
    let members = fetch_members_recursive(conn, &t_name)?;
    Ok(json!(members))
}

fn fetch_members_recursive(conn: &Connection, class_name: &str) -> anyhow::Result<Vec<Value>> {
    let mut result = Vec::new();
    let mut queue = vec![class_name.to_string()];
    let mut visited = HashMap::new();
    while let Some(current) = queue.pop() {
        if visited.contains_key(&current) { continue; }
        visited.insert(current.clone(), true);
        
        // Find all class IDs for this name (to handle multiple declarations/definitions)
        let mut stmt = conn.prepare("
            SELECT c.id FROM classes c 
            JOIN strings sc ON c.name_id = sc.id
            WHERE LOWER(sc.text) = LOWER(?)
        ")?;
        let class_ids: Vec<i64> = stmt.query_map([&current], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        for class_id in class_ids {
            let mut mem_stmt = conn.prepare("
                SELECT smn.text, smt.text, srt.text, access, is_static, detail, m.line_number, sp.text
                FROM members m 
                JOIN strings smn ON m.name_id = smn.id
                JOIN strings smt ON m.type_id = smt.id
                LEFT JOIN strings srt ON m.return_type_id = srt.id
                LEFT JOIN files f ON m.file_id = f.id
                LEFT JOIN strings sp ON f.path_id = sp.id
                WHERE class_id = ?
                ORDER BY smn.text ASC
            ")?;
            let mem_rows = mem_stmt.query_map([class_id], |row| {
                let m_name: String = row.get(0)?;
                let m_type: String = row.get(1)?;
                let r_type: Option<String> = row.get(2)?;
                let detail: Option<String> = row.get(5)?;
                let line: usize = row.get::<_, Option<usize>>(6)?.unwrap_or(0);
                let f_path: Option<String> = row.get(7).ok();

                let doc = if let Some(path) = f_path {
                    let mut comment = extract_comment_from_file(&path, line);
                    if let Some(d) = detail {
                        if !comment.is_empty() { comment.push_str("\n\n"); }
                        comment.push_str(&d);
                    }
                    comment
                } else {
                    detail.unwrap_or_default()
                };

                Ok(json!({ "label": m_name, "kind": map_kind(&m_type), "detail": r_type.unwrap_or_default(), "documentation": doc, "insertText": m_name }))
            })?;
            for m in mem_rows { result.push(m?); }
            let mut enum_stmt = conn.prepare("
                SELECT sen.text FROM enum_values ev 
                JOIN strings sen ON ev.name_id = sen.id
                WHERE enum_id = ?
            ")?;
            let enum_rows = enum_stmt.query_map([class_id], |row| {
                let e_name: String = row.get(0)?;
                Ok(json!({ "label": e_name, "kind": 20, "detail": "enum item", "insertText": e_name }))
            })?;
            for e in enum_rows { result.push(e?); }
            let mut parent_stmt = conn.prepare("
                SELECT si.text FROM inheritance i 
                JOIN strings si ON i.parent_name_id = si.id
                WHERE child_id = ?")?;
            let p_rows = parent_stmt.query_map([class_id], |row| Ok(row.get::<_, String>(0)?))?;
            for p in p_rows { queue.push(p?); }
        }
    }
    Ok(result)
}

fn fetch_global_symbols(conn: &Connection, prefix: &str) -> anyhow::Result<Value> {
    let mut results = Vec::new();
    let limit = 50;
    
    let mut stmt = conn.prepare("
        SELECT s.text, symbol_type FROM classes c
        JOIN strings s ON c.name_id = s.id
        WHERE LOWER(s.text) LIKE LOWER(?) AND symbol_type IN ('class', 'struct', 'enum')
        LIMIT ?
    ")?;
    let prefix_search = format!("{}%", prefix);
    let rows = stmt.query_map([&prefix_search, &limit.to_string()], |row| {
        let name: String = row.get(0)?;
        let sym_type: String = row.get(1)?;
        let kind = match sym_type.as_str() {
            "class" | "struct" => 7, // Class
            "enum" => 13, // Enum
            _ => 1
        };
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
            items.push(json!({ "label": "Category", "kind": 12, "detail": "Specifier", "documentation": "Specifies the category of the property in the Editor UI. Usage: Category=\"MyCategory\"" }));
            items.push(json!({ "label": "VisibleAnywhere", "kind": 12, "detail": "Specifier", "documentation": "Indicates that this property is visible in property windows, but cannot be edited." }));
            items.push(json!({ "label": "Transient", "kind": 12, "detail": "Specifier", "documentation": "Property is not saved to disk and not loaded from disk." }));
            items.push(json!({ "label": "Replicated", "kind": 12, "detail": "Specifier", "documentation": "Property should be replicated over the network." }));
        },
        "UFUNCTION" => {
            items.push(json!({ "label": "BlueprintCallable", "kind": 12, "detail": "Specifier", "documentation": "The function can be executed in a Blueprint or Level Blueprint graph." }));
            items.push(json!({ "label": "BlueprintPure", "kind": 12, "detail": "Specifier", "documentation": "The function does not affect the owning object in any way and can be executed in a Blueprint or Level Blueprint graph." }));
            items.push(json!({ "label": "Category", "kind": 12, "detail": "Specifier", "documentation": "Specifies the category of the function in the Editor UI." }));
            items.push(json!({ "label": "BlueprintImplementableEvent", "kind": 12, "detail": "Specifier", "documentation": "The function can be overridden in a Blueprint or Level Blueprint graph." }));
            items.push(json!({ "label": "BlueprintNativeEvent", "kind": 12, "detail": "Specifier", "documentation": "The function can be overridden in a Blueprint, but also has a native default implementation." }));
            items.push(json!({ "label": "Server", "kind": 12, "detail": "Specifier", "documentation": "The function is only executed on the server." }));
            items.push(json!({ "label": "Client", "kind": 12, "detail": "Specifier", "documentation": "The function is only executed on the client that owns the object." }));
        },
        "UCLASS" | "USTRUCT" => {
            items.push(json!({ "label": "Blueprintable", "kind": 12, "detail": "Specifier", "documentation": "Exposes this class as an acceptable base class for creating Blueprints." }));
            items.push(json!({ "label": "BlueprintType", "kind": 12, "detail": "Specifier", "documentation": "Exposes this class as a type that can be used for variables in Blueprints." }));
            items.push(json!({ "label": "Abstract", "kind": 12, "detail": "Specifier", "documentation": "Prevents the user from adding actors of this class to the world in the Editor." }));
            items.push(json!({ "label": "MinimalAPI", "kind": 12, "detail": "Specifier", "documentation": "Causes only the class's type information to be exported for use by other modules." }));
        },
        _ => return None
    }
    
    if items.is_empty() { None } else { Some(json!(items)) }
}

fn map_kind(k: &str) -> i64 {
    match k { "function" => 2, "variable" | "property" => 5, "enum_item" => 20, _ => 1 }
}

fn extract_comment_from_file(file_path: &str, line_number: usize) -> String {
    if line_number == 0 { return String::new(); }
    let content = match std::fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    let lines: Vec<&str> = content.lines().collect();
    if line_number > lines.len() { return String::new(); }

    let mut comment_lines = Vec::new();
    let mut current_line = line_number - 1; // 0-indexed, and we want the line *before* the definition

    // Skip empty lines or attributes [ ... ] or macros like UPROPERTY(...)
    while current_line > 0 {
        let trimmed = lines[current_line - 1].trim();
        if trimmed.is_empty() || trimmed.starts_with('[') || trimmed.starts_with("UPROPERTY") || trimmed.starts_with("UFUNCTION") || trimmed.starts_with("GENERATED_BODY") {
            current_line -= 1;
            continue;
        }
        break;
    }

    // Now look for comments
    let mut in_block_comment = false;
    while current_line > 0 {
        let trimmed = lines[current_line - 1].trim();
        if trimmed.starts_with("//") {
            comment_lines.push(trimmed.trim_start_matches('/').trim_start_matches('/').trim().to_string());
            current_line -= 1;
        } else if trimmed.ends_with("*/") {
            in_block_comment = true;
            let content = trimmed.trim_end_matches("*/");
            if content.starts_with("/*") {
                comment_lines.push(content.trim_start_matches("/*").trim_start_matches('*').trim().to_string());
                break;
            }
            comment_lines.push(content.trim_start_matches('*').trim().to_string());
            current_line -= 1;
        } else if in_block_comment {
            if trimmed.starts_with("/*") {
                comment_lines.push(trimmed.trim_start_matches("/*").trim_start_matches('*').trim().to_string());
                break;
            }
            comment_lines.push(trimmed.trim_start_matches('*').trim().to_string());
            current_line -= 1;
        } else {
            break;
        }
    }

    comment_lines.reverse();
    comment_lines.join("\n")
}

fn is_known_type(conn: &Connection, name: &str) -> anyhow::Result<bool> {
    let clean = extract_clean_type(name);
    if clean.is_empty() { return Ok(false); }
    let mut stmt = conn.prepare("
        SELECT 1 FROM classes c 
        JOIN strings s ON c.name_id = s.id
        WHERE LOWER(s.text) = LOWER(?) LIMIT 1
    ")?;
    Ok(stmt.exists([&clean])?)
}

fn infer_variable_type(conn: &Connection, target_name: &str, root: &Node, content: &str, cursor_row: usize) -> anyhow::Result<Option<String>> {
    tracing::info!("infer_variable_type(target='{}', cursor_row={})", target_name, cursor_row);
    let language: tree_sitter::Language = tree_sitter_unreal_cpp::LANGUAGE.into();
    let query_str = "
      (declaration type: (_) @type declarator: (init_declarator declarator: (_) @decl))
      (declaration type: (_) @type declarator: (_) @decl)
      (field_declaration type: (_) @type declarator: (init_declarator declarator: (_) @decl))
      (field_declaration type: (_) @type declarator: (_) @decl)
      (parameter_declaration type: (_) @type declarator: (_) @decl)
      (for_range_loop type: (_) @type declarator: (_) @decl)
      (condition_clause value: (declaration type: (_) @type declarator: (init_declarator declarator: (_) @decl)))
      (condition_clause value: (declaration type: (_) @type declarator: (_) @decl))
      (condition_clause (declaration type: (_) @type declarator: (_) @decl))
      (field_declaration (_) @type (init_declarator declarator: (_) @decl))
      (field_declaration (_) @type (_) @decl)
      (declaration (_) @type (init_declarator declarator: (_) @decl))
    ";
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
                    tracing::info!("Found match for '{}' at row {} (Type Kind: {})", target_name, row, t_node.kind());
                    if row <= cursor_row && (best_type.is_none() || row >= best_row) {
                        let type_text = get_node_text(&t_node, content).trim();
                        if type_text == "auto" {
                            if let Some(inferred) = infer_from_assignment(conn, target_name, root, content, cursor_row)? {
                                tracing::info!("Inferred 'auto' type: '{}'", inferred);
                                best_type = Some(inferred);
                            }
                        } else {
                            best_type = Some(extract_clean_type(type_text));
                        }
                        best_row = row;
                    }
                }
            }
        }
    }
    if best_type.is_none() {
        tracing::info!("No direct declaration found, trying broad assignment inference...");
        best_type = infer_from_assignment(conn, target_name, root, content, cursor_row)?;
    }
    tracing::info!("infer_variable_type Result for '{}': {:?}", target_name, best_type);
    Ok(best_type)
}

fn find_identifier_in_decl(node: &Node, target_name: &str, content: &str) -> anyhow::Result<bool> {
    let kind = node.kind();
    if kind == "identifier" || kind == "field_identifier" {
        return Ok(get_node_text(node, content).trim() == target_name);
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            // IMPORTANT: Skip 'value' or 'initializer' fields for ANY node type
            if let Some(field_name) = node.field_name_for_child(i as u32) {
                if field_name == "value" || field_name == "initializer" {
                    continue;
                }
            }
            if find_identifier_in_decl(&child, target_name, content)? { return Ok(true); }
        }
    }
    Ok(false)
}

fn infer_from_assignment(conn: &Connection, target_name: &str, root: &Node, content: &str, cursor_row: usize) -> anyhow::Result<Option<String>> {
    tracing::info!("infer_from_assignment(target='{}')", target_name);
    let language: tree_sitter::Language = tree_sitter_unreal_cpp::LANGUAGE.into();
    let query_str = "
      (declaration type: (_) declarator: (init_declarator declarator: (_) @decl value: (_) @value))
      (declaration declarator: (_) @decl value: (_) @value)
      (condition_clause value: (declaration type: (_) declarator: (init_declarator declarator: (_) @decl value: (_) @value)))
      (condition_clause value: (declaration declarator: (_) @decl value: (_) @value))
      (assignment_expression left: (_) @decl right: (_) @value)
    ";
    let query = Query::new(&language, query_str)?;
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, *root, content.as_bytes());
    while let Some(m) = matches.next() {
        let mut decl_node = None;
        let mut value_node = None;
        for cap in m.captures {
            let c_name = query.capture_names()[cap.index as usize];
            if c_name == "decl" { decl_node = Some(cap.node); }
            else if c_name == "value" { value_node = Some(cap.node); }
        }
        if let (Some(d_node), Some(v_node)) = (decl_node, value_node) {
            // ONLY infer if the target IS the direct subject of assignment (or pointer/ref to it)
            // Skip if it's a subscript_expression or field_expression
            let dk = d_node.kind();
            if dk == "subscript_expression" || dk == "field_expression" {
                continue;
            }

            let found_name = if dk == "identifier" { get_node_text(&d_node, content).trim() } else { "" };
            if !found_name.is_empty() && found_name == target_name {
                let row = d_node.start_position().row;
                if row <= cursor_row { 
                    tracing::info!("Found assignment match for '{}' at row {} (Value Kind: {}), resolving RHS...", target_name, row, v_node.kind());
                    if let Ok(Some(t)) = resolve_expression_type(conn, v_node, root, content, cursor_row) {
                        tracing::info!("RHS resolved successfully: '{}'", t);
                        return Ok(Some(t));
                    }
                    let v_text = get_node_text(&v_node, content);
                    tracing::info!("RHS resolve failed, fallback to regex on: '{}'", v_text);
                    return infer_from_value_text(v_text);
                }
            } else if find_identifier_in_decl(&d_node, target_name, content)? {
                // Handle complex but direct declarators like *x or &x
                let row = d_node.start_position().row;
                if row <= cursor_row {
                    tracing::info!("Found complex assignment match for '{}' at row {}, resolving RHS...", target_name, row);
                    if let Ok(Some(t)) = resolve_expression_type(conn, v_node, root, content, cursor_row) {
                        return Ok(Some(t));
                    }
                }
            }
        }
    }
    Ok(None)
}

fn infer_from_value_text(text: &str) -> anyhow::Result<Option<String>> {
    let text = text.trim();
    if let Ok(re) = regex::Regex::new(r"CreateDefaultSubobject\s*<\s*([a-zA-Z0-9_:]+)") {
        if let Some(cap) = re.captures(text) { 
            return Ok(Some(extract_clean_type(cap.get(1).unwrap().as_str())));
        }
    }
    if let Ok(re) = regex::Regex::new(r"([a-zA-Z0-9_]+)\s*<\s*([a-zA-Z0-9_:]+)") {
        if let Some(cap) = re.captures(text) { 
            let func = cap.get(1).unwrap().as_str();
            let inner = cap.get(2).unwrap().as_str();
            if ["NewObject", "TObjectPtr", "TSharedPtr"].contains(&func) {
                return Ok(Some(extract_clean_type(inner)));
            }
            return Ok(Some(extract_clean_type(func)));
        }
    }
    if let Ok(re) = regex::Regex::new(r"^([a-zA-Z0-9_:]+)\s*\(") {
        if let Some(cap) = re.captures(text) { return Ok(Some(extract_clean_type(cap.get(1).unwrap().as_str()))); }
    }
    Ok(None)
}

fn extract_clean_type(raw: &str) -> String {
    let mut clean = raw.trim().to_string();
    
    // 1. Remove qualifiers/keywords first so they don't interfere with template name matching
    let keywords = ["const", "typename", "struct", "class", "enum", "virtual", "static", "inline", "FORCEINLINE"];
    for kw in keywords {
        if let Ok(re) = regex::Regex::new(&format!(r"\b{}\b", kw)) { 
            clean = re.replace_all(&clean, "").to_string(); 
        }
    }
    if let Ok(re) = regex::Regex::new(r"\b[A-Z0-9_]+_API\b") { 
        clean = re.replace_all(&clean, "").to_string(); 
    }
    clean = clean.trim().to_string();

    // 2. Handle template unwrapping
    if let Some(start) = clean.find('<') {
        if let Some(end) = clean.rfind('>') {
            let wrapper = clean[..start].trim();
            let inner = &clean[start+1..end];
            if ["TObjectPtr", "TSharedPtr", "TUniquePtr", "TWeakObjectPtr", "TSubclassOf", "TSoftObjectPtr", "TSoftClassPtr", "TEnumAsByte"].contains(&wrapper) {
                return extract_clean_type(inner);
            }
            // For other templates like TArray/TMap, return the whole cleaned string (including <...>)
            // but first remove pointers/refs from the outer wrapper if any (e.g. TArray<int>*)
            let outer = wrapper.replace('*', "").replace('&', "").trim().to_string();
            let suffix_cleaned = clean[end+1..].replace('*', "").replace('&', "").trim().to_string();
            return format!("{}<{}>{}", outer, inner, suffix_cleaned).trim().to_string();
        }
    }

    // 3. Remove pointers/references and take the last part
    clean = clean.replace('*', " ").replace('&', " ");
    clean.split_whitespace().last().unwrap_or("").split("::").last().unwrap_or("").to_string()
}
