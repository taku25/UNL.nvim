use rusqlite::{Connection, params, OptionalExtension};
use serde_json::{json, Value};
use tree_sitter::{Parser, Point, Node, Query, QueryCursor, StreamingIterator};
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::Mutex;
use crate::server::state::CompletionCache;

struct RequestContext<'a> {
    conn: &'a Connection,
    file_cache: HashMap<String, Vec<String>>,
    inheritance_cache: HashMap<(String, String), bool>,
    string_id_cache: HashMap<String, i64>,
    /// 現在編集中ファイルのDB上のfile_id（遅延取得）
    current_file_id: Option<i64>,
    /// 現在ファイルから（推移的に）includeされているfile_idのセット（遅延取得）
    included_file_ids: Option<std::collections::HashSet<i64>>,
}

impl<'a> RequestContext<'a> {
    fn new(conn: &'a Connection) -> Self {
        Self {
            conn,
            file_cache: HashMap::new(),
            inheritance_cache: HashMap::new(),
            string_id_cache: HashMap::new(),
            current_file_id: None,
            included_file_ids: None,
        }
    }

    fn get_string_id(&mut self, text: &str) -> anyhow::Result<Option<i64>> {
        let text = text.trim();
        if text.is_empty() { return Ok(None); }
        if let Some(&id) = self.string_id_cache.get(text) {
            return Ok(Some(id));
        }
        // 厳密な比較 (=) に戻す。インデックスが最適に効く。
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

    fn get_class_ids_by_name(&mut self, class_name: &str) -> anyhow::Result<Vec<i64>> {
        let class_name = class_name.trim();
        if class_name.is_empty() { return Ok(Vec::new()); }

        // 1. まずは完全一致（UE::Math::TVector などが一個の文字列で登録されている可能性）
        if let Some(name_id) = self.get_string_id(class_name)? {
            let mut stmt = self.conn.prepare("SELECT id FROM classes WHERE name_id = ?")?;
            let ids: Vec<i64> = stmt.query_map([name_id], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            if !ids.is_empty() { return Ok(ids); }
        }

        // 2. 名前空間で分割して検索（UE::Math と TVector に分かれている可能性）
        if class_name.contains("::") {
            let parts: Vec<&str> = class_name.split("::").collect();
            if parts.len() >= 2 {
                let ns_name = parts[..parts.len()-1].join("::");
                let cls_name = parts[parts.len()-1];
                
                if let (Some(ns_id), Some(cls_id)) = (self.get_string_id(&ns_name)?, self.get_string_id(cls_name)?) {
                    let mut stmt = self.conn.prepare("SELECT id FROM classes WHERE name_id = ? AND namespace_id = ?")?;
                    let ids: Vec<i64> = stmt.query_map([cls_id, ns_id], |row| row.get(0))?
                        .filter_map(|r| r.ok())
                        .collect();
                    if !ids.is_empty() { return Ok(ids); }
                }
                
                // 名前空間を無視して名前だけで検索（フォールバック）
                return self.get_class_ids_by_name(cls_name);
            }
        }

        Ok(Vec::new())
    }

    /// 現在ファイルからincludeされているファイルIDセットを取得（BFS、推移的）
    fn get_included_file_ids(&mut self) -> &std::collections::HashSet<i64> {
        if self.included_file_ids.is_none() {
            let mut set = std::collections::HashSet::new();
            if let Some(root_id) = self.current_file_id {
                let mut queue = vec![root_id];
                while let Some(fid) = queue.pop() {
                    if !set.insert(fid) { continue; }
                    // このファイルが直接includeしているファイルを取得
                    if let Ok(mut stmt) = self.conn.prepare_cached(
                        "SELECT resolved_file_id FROM file_includes WHERE file_id = ? AND resolved_file_id IS NOT NULL"
                    ) {
                        if let Ok(rows) = stmt.query_map([fid], |row| row.get::<_, i64>(0)) {
                            for r in rows.flatten() {
                                if !set.contains(&r) {
                                    queue.push(r);
                                }
                            }
                        }
                    }
                }
            }
            self.included_file_ids = Some(set);
        }
        self.included_file_ids.as_ref().unwrap()
    }

    /// 同名クラスが複数ある場合、現在ファイルのinclude階層内にあるものを優先して絞り込む
    fn filter_class_ids_by_includes(&mut self, ids: Vec<i64>) -> Vec<i64> {
        if ids.len() <= 1 || self.current_file_id.is_none() { return ids; }

        // 各クラスのfile_idを取得
        let with_file: Vec<(i64, Option<i64>)> = ids.iter().filter_map(|&cid| {
            self.conn.query_row("SELECT file_id FROM classes WHERE id = ?", [cid], |r| r.get::<_, Option<i64>>(0)).ok()
                .map(|fid| (cid, fid))
        }).collect();

        let included = self.get_included_file_ids();
        let filtered: Vec<i64> = with_file.iter()
            .filter(|(_, fid)| fid.map(|f| included.contains(&f)).unwrap_or(false))
            .map(|(cid, _)| *cid)
            .collect();

        if filtered.is_empty() { ids } else { filtered }
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
    tracing::debug!("--- Completion Request at {}:{} ---", line, character);
    let mut ctx = RequestContext::new(conn);

    // 現在ファイルのfile_idをDBから引いておく（include-aware disambiguation用）
    // ファイル名だけでなくフルパス（ディレクトリ階層）で一致させる
    if let Some(ref fp) = _file_path {
        ctx.current_file_id = get_file_id_by_full_path(conn, fp);
        tracing::debug!("Current file '{}' resolved to file_id: {:?}", fp, ctx.current_file_id);
    }
    
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
            tracing::debug!("No node found at cursor position.");
            return Ok(json!([]));
        }
    };

    let node_type = node.kind();
    tracing::debug!("Node at cursor: kind='{}', text='{}'", node_type, get_node_text(&node, content));
    
    // Check if we are inside or near an ERROR node
    let mut target_node = None;
    if node_type == "ERROR" || node_type == "." || node_type == "->" || node_type == "::" {
        if let Some(prev) = get_prev_meaningful_sibling(node) {
            tracing::debug!("Found meaningful sibling before ERROR/Operator: kind='{}'", prev.kind());
            target_node = Some(prev);
        } else if let Some(parent) = node.parent() {
            if parent.kind() == "ERROR" {
                if let Some(prev) = get_prev_meaningful_sibling(parent) {
                    tracing::debug!("Found meaningful sibling before parent ERROR: kind='{}'", prev.kind());
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
            tracing::debug!("Operator detected (Case 1), target node: kind='{}', text='{}'", prev.kind(), get_node_text(&prev, content));
            return resolve_node_and_fetch_members(&mut ctx, prev, &root, content, row, None, cache, persistent_cache);
        } else {
            tracing::debug!("Operator detected but no meaningful sibling found. Continuing to traverse up from parent.");
            curr_opt = node.parent(); // Move to parent and let Case 2 handle it
        }
    }

    // 2. 識別子の入力途中、またはそれ以外
    while let Some(curr) = curr_opt {
        let p_kind = curr.kind();
        tracing::debug!("Traversing up: kind='{}', text='{}'", p_kind, get_node_text(&curr, content));

        if p_kind == "unreal_argument_list" || p_kind == "macro_argument_list" {
            if let Some(parent) = curr.parent() {
                let full_text = get_node_text(&parent, content);
                let macro_name_key = full_text.split('(').next().unwrap_or("").trim();
                if let Some(res) = resolve_macro_specifiers(macro_name_key) {
                    tracing::debug!("Resolved macro specifiers for '{}'", macro_name_key);
                    return Ok(res);
                }
                if let Some(grand) = parent.parent() {
                   let g_text = get_node_text(&grand, content);
                   let g_key = g_text.split('(').next().unwrap_or("").trim();
                   if let Some(res) = resolve_macro_specifiers(g_key) {
                       tracing::debug!("Resolved macro specifiers for grand '{}'", g_key);
                       return Ok(res);
                   }
                }
            }
        }

        // meta=(...) のネストされたスペシファイアリストの中にいる場合
        if p_kind == "unreal_specifier_list" {
            if let Some(spec_content) = curr.parent() {
                if spec_content.kind() == "unreal_specifier_content" {
                    if let Some(spec_node) = spec_content.parent() {
                        if spec_node.kind() == "unreal_specifier" {
                            if let Some(key_node) = spec_node.child_by_field_name("key") {
                                let key_text = get_node_text(&key_node, content).to_lowercase();
                                if key_text == "meta" {
                                    tracing::debug!("Resolved meta specifiers (nested meta=(...))");
                                    return Ok(json!(resolve_meta_specifiers()));
                                }
                            }
                        }
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
                tracing::debug!("Field expression detected (Case 2), resolving argument with prefix: {:?}", field_prefix);
                return resolve_node_and_fetch_members(&mut ctx, obj_node, &root, content, row, field_prefix, cache, persistent_cache);
            } else if let Some(first_child) = curr.child(0) {
                if first_child.kind() != "." && first_child.kind() != "->" {
                    tracing::debug!("Field expression detected (Fallback), resolving first child...");
                    return resolve_node_and_fetch_members(&mut ctx, first_child, &root, content, row, field_prefix, cache, persistent_cache);
                }
            }
        } else if p_kind == "call_expression" && (node_type == "." || node_type == "->") {
             if let Some(func_node) = curr.child_by_field_name("function") {
                 tracing::debug!("Call expression parent of operator detected, resolving function...");
                 return resolve_node_and_fetch_members(&mut ctx, func_node, &root, content, row, None, cache, persistent_cache);
             }
        } else if p_kind == "qualified_identifier" {
            let field_prefix = curr.child_by_field_name("name").map(|name_node| get_node_text(&name_node, content).to_string());

            if let Some(scope_node) = curr.child_by_field_name("scope") {
                tracing::debug!("Qualified identifier detected (Case 2), resolving scope with prefix: {:?}", field_prefix);
                return resolve_static_members(&mut ctx, get_node_text(&scope_node, content), field_prefix, cache, persistent_cache);
            }
        } else if p_kind == "ERROR" {
            let count = curr.child_count();
            for i in (0..count).rev() {
                if let Some(child) = curr.child(i as u32) {
                    let ck = child.kind();
                    if ck == "." || ck == "->" || ck == "::" {
                        if let Some(prev) = get_prev_meaningful_sibling(child) {
                             tracing::debug!("Operator detected inside ERROR, resolving previous sibling...");
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
            if let Ok(members) = fetch_members_recursive(&mut ctx, &current_class, Some(prefix.to_string()), cache.as_ref().map(Arc::clone), persistent_cache.clone(), Some(&current_class)) {
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

#[allow(clippy::too_many_arguments)]
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
        tracing::debug!("Final type for member lookup: '{}', current_class: {:?}, prefix: {:?}", resolved, current_class, prefix);
        
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
    tracing::debug!("resolve_expression_type(kind='{}', text='{}')", kind, get_node_text(&node, content));

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
                return inner.trim().to_string();
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
    // Smart pointer passthrough: TWeakObjectPtr<T>::Get() → T  (before extract_clean_type strips the wrapper)
    const SMART_PTRS: &[&str] = &[
        "TWeakObjectPtr", "TObjectPtr", "TSharedPtr", "TSharedRef",
        "TUniquePtr", "TWeakPtr", "TStrongObjectPtr", "TSoftObjectPtr",
    ];
    let raw = class_name.trim();
    for sp in SMART_PTRS {
        if raw.starts_with(sp) {
            if let (Some(lt), Some(gt)) = (raw.find('<'), raw.rfind('>')) {
                let inner = raw[lt + 1..gt].trim();
                match member_name {
                    "Get" | "GetChecked" | "operator*" | "operator->" => {
                        return Ok(Some(extract_clean_type(inner)));
                    }
                    "IsValid" | "IsStale" | "IsExplicitlyNull" => {
                        return Ok(Some("bool".to_string()));
                    }
                    _ => {}
                }
            }
        }
    }

    let clean_class = extract_clean_type(class_name);
    let resolved_class = resolve_typedef(ctx, &clean_class)?;
    
    let start_class_ids = ctx.get_class_ids_by_name(&resolved_class)?;
    if start_class_ids.is_empty() { return Ok(None); }

    let mut queue = start_class_ids;
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
            } else {
                let ids = ctx.get_class_ids_by_name(&p_name)?;
                for id in ids { queue.push(id); }
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
    for _ in 0..5 {
        // Case 1: ClassName::TypeAlias → look up the alias as a type_alias member of the class
        if current.contains("::") {
            let parts: Vec<&str> = current.splitn(2, "::").collect();
            if parts.len() == 2 {
                let class_part = parts[0].trim();
                let alias_part = parts[1].trim();
                if let (Some(class_name_id), Some(alias_name_id)) = (
                    ctx.get_string_id(class_part)?,
                    ctx.get_string_id(alias_part)?,
                ) {
                    let mut stmt = ctx.conn.prepare("
                        SELECT sr.text FROM members m
                        LEFT JOIN strings sr ON m.return_type_id = sr.id
                        WHERE m.class_id IN (SELECT id FROM classes WHERE name_id = ?)
                          AND m.name_id = ?
                          AND m.type_id = (SELECT id FROM strings WHERE text = 'type_alias')
                        LIMIT 1
                    ")?;
                    let mut rows = stmt.query(params![class_name_id, alias_name_id])?;
                    if let Some(row) = rows.next()? {
                        if let Some(resolved) = row.get::<_, Option<String>>(0)? {
                            let clean = extract_clean_type(&resolved);
                            tracing::debug!("resolve_typedef: '{}' → '{}' (via class type_alias member)", current, clean);
                            if clean.is_empty() || clean == current { break; }
                            current = clean;
                            continue;
                        }
                    }
                }
            }
        }

        // Case 2: global typedef class entry
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

    let child_ids = ctx.get_class_ids_by_name(child)?;
    let parent_ids = ctx.get_class_ids_by_name(parent)?;

    let mut queue = child_ids;
    let mut visited = HashMap::new();
    let mut found = false;
    while let Some(current_id) = queue.pop() {
        if parent_ids.contains(&current_id) { found = true; break; }
        
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
            } else {
                let ids = ctx.get_class_ids_by_name(&p_name)?;
                for id in ids { queue.push(id); }
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
        let mut c = c_mutex.lock();
        if let Some(cached) = c.get(&cache_key, "") {
            if let Some(arr) = cached.as_array() {
                return Ok(arr.clone());
            }
        }
    }

    // 2. Try Persistent Cache
    if let Some(pc_mutex) = &persistent_cache {
        let pc = pc_mutex.lock();
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

    // テンプレート引数付きの型名（例: TArray<FGameplayTag>）はDBにそのまま登録されていないことが多い。
    // まずフル名で検索し、見つからなければ '<' 以前のベース名（例: TArray）で再試行する。
    let mut start_class_ids = ctx.get_class_ids_by_name(class_name)?;
    if start_class_ids.is_empty() {
        if let Some(base) = class_name.split('<').next() {
            let base = base.trim();
            if !base.is_empty() && base != class_name {
                start_class_ids = ctx.get_class_ids_by_name(base)?;
            }
        }
    }
    // 同名クラスが複数ある場合（standalone_prologue.h等のスタブ vs 本物）、
    // 現在ファイルのinclude階層内にあるものを優先して絞り込む
    start_class_ids = ctx.filter_class_ids_by_includes(start_class_ids);
    if start_class_ids.is_empty() { return Ok(Vec::new()); }

    let mut result = Vec::new();
    let mut seen_members: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    let mut queue = start_class_ids;
    let mut visited = HashMap::new();
    // 同名クラスが複数IDある場合（.hと複数.cpp実装）に親クラスが重複キューされないよう
    // クラス名単位でも訪問済みを追跡する
    let mut visited_class_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    visited_class_names.insert(class_name.to_string());
    let prefix_search = prefix.as_ref().map(|p| format!("{}%", p));

    while let Some(current_class_id) = queue.pop() {
        if visited.contains_key(&current_class_id) { continue; }
        visited.insert(current_class_id, true);
        
        let mut sql = format!("
            {}
            SELECT smn.text, smt.text, srt.text, access, detail, m.line_number, dp.full_path || '/' || sn.text
            FROM members m 
            JOIN strings smn ON m.name_id = smn.id
            JOIN strings smt ON m.type_id = smt.id
            LEFT JOIN strings srt ON m.return_type_id = srt.id
            LEFT JOIN files f ON m.file_id = f.id
            LEFT JOIN dir_paths dp ON f.directory_id = dp.id
            LEFT JOIN strings sn ON f.filename_id = sn.id
            WHERE m.class_id = ?", crate::db::path::PATH_CTE);
        
        if prefix_search.is_some() {
            sql.push_str(" AND smn.text LIKE ?");
        }
        sql.push_str(" AND (m.access IS NULL OR m.access != 'impl') ORDER BY smn.text ASC LIMIT 200");

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

            let ret = r_type.unwrap_or_default();
            let dedup_key = (m_name.clone(), ret.clone());
            if seen_members.contains(&dedup_key) { continue; }
            seen_members.insert(dedup_key);

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
                "detail": ret, 
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
            // 同名クラスを複数エントリ経由で重複キューしない（BFSの指数的膨張を防ぐ）
            if visited_class_names.contains(&p_name) { continue; }
            visited_class_names.insert(p_name.clone());
            if let Some(id) = p_id {
                if !visited.contains_key(&id) {
                    queue.push(id);
                }
            }
            // IDがある場合でも、名前で再検索して他のID（前方宣言など）も網羅する
            let ids = ctx.get_class_ids_by_name(&p_name)?;
            for id in ids {
                if !visited.contains_key(&id) {
                    queue.push(id);
                }
            }
        }
        if result.len() >= 2000 { break; }
    }

    let result_json = json!(result);

    // 3. Store in Memory Cache
    if let Some(c_mutex) = &cache {
        let mut c = c_mutex.lock();
        c.put(&cache_key, "", result_json.clone());
    }

    // 4. Store in Persistent Cache
    if let Some(pc_mutex) = &persistent_cache {
        let pc = pc_mutex.lock();
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

fn spec(label: &str, doc: &str) -> Value {
    json!({ "label": label, "kind": 12, "detail": "Specifier", "documentation": doc })
}
fn spec_kv(label: &str, insert: &str, doc: &str) -> Value {
    json!({ "label": label, "kind": 12, "detail": "Specifier", "documentation": doc, "insertText": insert, "insertTextFormat": 2 })
}

fn resolve_meta_specifiers() -> Vec<Value> {
    vec![
        spec_kv("Tooltip", "Tooltip=\"$1\"", "Tooltip shown when hovering over the property in the editor."),
        spec_kv("DisplayName", "DisplayName=\"$1\"", "Overrides the display name of the property in the editor."),
        spec_kv("Category", "Category=\"$1\"", "Override the category for this property in the editor."),
        spec_kv("Keywords", "Keywords=\"$1\"", "Keywords used to find the property in search."),
        spec_kv("Units", "Units=\"$1\"", "Specifies the units of the property value (e.g. cm, kg, s)."),
        spec_kv("ClampMin", "ClampMin=\"$1\"", "Minimum allowed value for numeric properties."),
        spec_kv("ClampMax", "ClampMax=\"$1\"", "Maximum allowed value for numeric properties."),
        spec_kv("UIMin", "UIMin=\"$1\"", "Minimum value shown in the editor slider."),
        spec_kv("UIMax", "UIMax=\"$1\"", "Maximum value shown in the editor slider."),
        spec_kv("EditCondition", "EditCondition=\"$1\"", "Condition expression that enables/disables editing of this property."),
        spec("EditConditionHides", "Hide the property entirely when EditCondition is false."),
        spec("InlineEditConditionToggle", "The bool property acts as an inline toggle for EditCondition."),
        spec("AllowPrivateAccess", "Allows access from Blueprints even when declared in a private scope."),
        spec("AllowAbstract", "Allow abstract classes to be selected in object reference pickers."),
        spec("ExactClass", "Only show objects of exactly this class, not derived classes."),
        spec("NoSpinbox", "Disable the spinbox widget for numeric properties."),
        spec("ShowOnlyInnerProperties", "Show the inner properties of a struct directly without a header."),
        spec("FullyExpand", "Fully expand the property in the details panel."),
        spec("MultiLine", "Allow multi-line text input for FString/FText properties."),
        spec("PasswordField", "Show the text property as a password field (obscured input)."),
        spec("HideInDetailPanel", "Hide this property from the details panel."),
        spec("HideViewOptions", "Hide the view options button in asset pickers."),
        spec("ShowTreeView", "Show asset pickers as a tree view."),
        spec("BindWidget", "Binds this property to a named widget in a UMG widget blueprint."),
        spec("BindWidgetOptional", "Optionally binds this property to a named widget in a UMG widget blueprint."),
        spec("BindWidgetAnim", "Binds this property to a widget animation in a UMG widget blueprint."),
        spec_kv("TitleProperty", "TitleProperty=\"$1\"", "Property name to use as the title for array/set elements in the editor."),
        spec("ContentDir", "This FDirectoryPath property is relative to the Content directory."),
        spec("RelativePath", "This FFilePath property shows a relative path picker."),
        spec("RelativeToGameDir", "This FDirectoryPath property is relative to the Game directory."),
        spec_kv("FilePathFilter", "FilePathFilter=\"$1\"", "File extension filter for FFilePath properties."),
        spec_kv("MustImplement", "MustImplement=\"$1\"", "Interface that the selected class must implement."),
        spec("GetByRef", "Return a const reference instead of a copy when accessed from Blueprints."),
    ]
}

fn resolve_macro_specifiers(macro_name: &str) -> Option<Value> {
    let name = macro_name.split('(').next().unwrap_or("").trim();
    let items: Vec<Value> = match name {
        "UPROPERTY" => vec![
            // Visibility / editability
            spec("EditAnywhere",       "Can be edited by property windows in the editor, on instances and archetypes."),
            spec("EditDefaultsOnly",   "Can be edited only on archetypes/defaults, not on instances."),
            spec("EditInstanceOnly",   "Can be edited only on instances, not on archetypes/defaults."),
            spec("EditFixedSize",      "For dynamic arrays: disables adding/removing elements, but allows editing existing elements."),
            spec("VisibleAnywhere",    "Visible in all property windows (editor and instances) but not editable."),
            spec("VisibleDefaultsOnly","Visible on archetypes/defaults, not on instances. Not editable."),
            spec("VisibleInstanceOnly","Visible on instances, not on archetypes/defaults. Not editable."),
            // Blueprint access
            spec("BlueprintReadOnly",  "Can be read from Blueprints but not modified."),
            spec("BlueprintReadWrite", "Can be read and written from Blueprints."),
            spec_kv("BlueprintSetter", "BlueprintSetter=\"$1\"", "Designates a custom setter function to be called when this property is set in a Blueprint."),
            spec_kv("BlueprintGetter", "BlueprintGetter=\"$1\"", "Designates a custom getter function to be called when this property is read in a Blueprint."),
            spec("BlueprintAssignable","Multicast delegates only — can be assigned in Blueprints."),
            spec("BlueprintCallable",  "Multicast delegates only — can be called in Blueprints."),
            spec("BlueprintAuthorityOnly","Only fires in Blueprints running on a machine with network authority."),
            // Category / Meta
            spec_kv("Category",        "Category=\"$1\"",        "Specifies the category of the property in the editor UI."),
            spec_kv("meta",            "meta=($1)",              "Additional metadata specifiers for editor and Blueprint tooling."),
            // Replication
            spec("Replicated",         "This property will be replicated over the network."),
            spec_kv("ReplicatedUsing", "ReplicatedUsing=\"$1\"", "Specifies a callback function that is called when this property is received via replication."),
            spec("NotReplicated",      "Skip replication for this property in struct context. Struct properties are replicated by default."),
            // Serialization / Save
            spec("Transient",          "Property is not serialized; will be zero-filled at load time."),
            spec("DuplicateTransient", "Property is set to default value during duplication (e.g. copy-paste)."),
            spec("NonPIEDuplicateTransient","Property is reset to default when duplicated outside PIE."),
            spec("SaveGame",           "Include this property when saving a game via checkpoint or serialization."),
            spec("SkipSerialization",  "Property is not serialized but can still be exported."),
            spec("TextExportTransient","Transient for copy-paste export/import; will be reset to default."),
            // Asset registry
            spec("AssetRegistrySearchable","Property and its value will be automatically added to the Asset Registry for the containing asset."),
            // Misc flags
            spec("AdvancedDisplay",    "Move this property to the advanced dropdown in the details panel."),
            spec("SimpleDisplay",      "Always visible in the details panel (overrides advanced display)."),
            spec("Config",             "Value is loaded from the config file (.ini) and saved to it."),
            spec("GlobalConfig",       "Works like Config but the value cannot be overridden by subclasses."),
            spec("Instanced",          "Object properties only. Allows creating instances of sub-objects in the editor."),
            spec("Export",             "Object properties only. The sub-object referenced should be exported as a sub-object block (serialized inline)."),
            spec("NoClear",            "Disables the Clear button for object references in the editor."),
            spec("Interp",             "Indicates the value can be driven over time by a Matinee or Sequencer float track."),
            spec("NonTransactional",   "Changes to the value of this property are not included in the editor undo/redo transaction."),
            spec("WithSerializer",     "This property has a custom serializer. Used internally."),
        ],
        "UFUNCTION" => vec![
            spec("BlueprintCallable",            "Can be called from Blueprints and other visual scripting."),
            spec("BlueprintPure",                "Does not affect the owning object in any way; no output pin on execution path."),
            spec("BlueprintImplementableEvent",  "Base implementation is empty; Blueprint subclasses can override it."),
            spec("BlueprintNativeEvent",         "Designed to be overridden by a Blueprint, but has a native default implementation."),
            spec("Exec",                         "Can be called from in-game console commands."),
            spec("Server",                       "Called on the server only. Requires Reliable or Unreliable."),
            spec("Client",                       "Called on the owning client. Requires Reliable or Unreliable."),
            spec("NetMulticast",                 "Called on the server and all clients. Requires Reliable or Unreliable."),
            spec("Reliable",                     "Replicated function call is reliable (guaranteed delivery)."),
            spec("Unreliable",                   "Replicated function call may be dropped in bad network conditions."),
            spec("WithValidation",               "Declares a separate validation function (_Validate suffix) for network RPC."),
            spec("BlueprintAuthorityOnly",       "Only executes in Blueprints on the authority (server)."),
            spec("BlueprintCosmetic",            "Only executes in Blueprints on clients (never on dedicated server)."),
            spec("CallInEditor",                 "This function can be called from within the editor on selected instances via a button."),
            spec("CustomThunk",                  "Allows custom thunk function generation. Used for template functions exposed to Blueprints."),
            spec("SealedEvent",                  "This event cannot be overridden in Blueprint subclasses."),
            spec("ServiceRequest",               "An RPC function that is a service request."),
            spec("ServiceResponse",              "An RPC function that is a service response."),
            spec_kv("Category",                  "Category=\"$1\"", "Specifies the category in the Blueprint action menu."),
            spec_kv("meta",                      "meta=($1)",        "Additional metadata for Blueprint tooling."),
        ],
        "UCLASS" => vec![
            spec("Blueprintable",                "This class can be used as a base class for creating Blueprints."),
            spec("NotBlueprintable",             "This class cannot be used as a base class for Blueprints."),
            spec("BlueprintType",                "This class can be used as a variable type in Blueprints."),
            spec("Abstract",                     "Prevents direct instantiation. Must be subclassed."),
            spec("Placeable",                    "Can be placed in a level, in the UI Scene, or in a Blueprint."),
            spec("NotPlaceable",                 "Cannot be placed in editor views; overrides inherited Placeable."),
            spec("EditInlineNew",                "Supports creating new objects of this class from the editor property window."),
            spec("NotEditInlineNew",             "Disables inline creation of objects of this class."),
            spec("MinimalAPI",                   "Only exports the class type and its constructor. Sufficient for type-casting."),
            spec("Transient",                    "Objects of this class are never saved to disk."),
            spec("DefaultToInstanced",           "All instances of this class are considered 'instanced' by default."),
            spec("Deprecated",                   "This class is deprecated; objects will not be serialized."),
            spec("PerObjectConfig",              "Config information for this class will be stored per-object."),
            spec("ConfigDoNotCheckDefaults",     "Do not check the default value when reading config for this class."),
            spec("HideDropdown",                 "Suppress this class from combo-box class pickers in the editor."),
            spec("ComponentWrapperClass",        "Used to indicate this class wraps a component for a simpler interface in Blueprints."),
            spec_kv("Within",                    "Within=\"$1\"",    "Object of this class can only exist within an object of the given class."),
            spec_kv("Config",                    "Config=\"$1\"",    "Specifies the config file (.ini) this class's config properties use."),
            spec_kv("ClassGroup",                "ClassGroup=\"$1\"","Indicates the class will be displayed in a specific class group in the Actor Browser."),
            spec_kv("HideCategories",            "HideCategories=($1)","Hides specified property categories in the editor."),
            spec_kv("ShowCategories",            "ShowCategories=($1)","Overrides HideCategories to show specified categories."),
            spec_kv("AutoCollapseCategories",    "AutoCollapseCategories=($1)","Collapses the named categories by default."),
            spec_kv("AutoExpandCategories",      "AutoExpandCategories=($1)","Expands the named categories by default."),
            spec_kv("meta",                      "meta=($1)",        "Additional metadata for editor and Blueprint tooling."),
        ],
        "USTRUCT" => vec![
            spec("Atomic",      "Serialized as a single unit even when only individual members change."),
            spec("BlueprintType", "Can be used as a variable type in Blueprints."),
            spec("Immutable",   "Struct is immutable; it is an error to attempt to change any property values."),
            spec("NoExport",    "No auto-generated code will be created for this struct; requires manual declaration."),
            spec_kv("meta",     "meta=($1)", "Additional metadata for editor and Blueprint tooling."),
        ],
        "UENUM" => vec![
            spec("BlueprintType", "This enum can be used as a variable type in Blueprints."),
            spec_kv("meta",     "meta=($1)", "Additional metadata for editor and Blueprint tooling."),
        ],
        "UINTERFACE" => vec![
            spec("Blueprintable",  "This interface can be implemented by Blueprints."),
            spec("BlueprintType",  "This interface can be used as a variable type in Blueprints."),
            spec("MinimalAPI",     "Only export type information for this interface."),
            spec_kv("meta",        "meta=($1)", "Additional metadata for editor and Blueprint tooling."),
        ],
        _ => return None,
    };
    if items.is_empty() { None } else { Some(json!(items)) }
}

fn map_kind(k: &str) -> i64 {
    match k { "function" => 2, "variable" | "property" => 5, "enum_item" => 20, "type_alias" => 7, _ => 1 }
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
        let mut stmt = ctx.conn.prepare("SELECT 1 FROM classes WHERE name_id = ? LIMIT 1")?;
        return Ok(stmt.exists([id])?);
    }
    Ok(false)
}

fn infer_variable_type(ctx: &mut RequestContext, target_name: &str, root: &Node, content: &str, cursor_row: usize) -> anyhow::Result<Option<String>> {
    let language: tree_sitter::Language = tree_sitter_unreal_cpp::LANGUAGE.into();
    // Note: for_range_loop の _declaration_specifiers は anonymous rule のため type: フィールドが
    // grammar.js には直接書かれていないが、node-types.json では type: が存在する。
    // for_range_loop パターンは `declarator:` のみキャプチャし、type_node = None の場合に
    // infer_for_range_element_type へ分岐する。
    let query_str = "(declaration type: (_) @type declarator: (init_declarator declarator: (_) @decl)) (declaration type: (_) @type declarator: (_) @decl) (field_declaration type: (_) @type declarator: (init_declarator declarator: (_) @decl)) (field_declaration type: (_) @type declarator: (_) @decl) (parameter_declaration type: (_) @type declarator: (_) @decl) (for_range_loop declarator: (_) @decl) (condition_clause value: (declaration type: (_) @type declarator: (init_declarator declarator: (_) @decl))) (condition_clause value: (declaration type: (_) @type declarator: (_) @decl)) (condition_clause (declaration type: (_) @type declarator: (init_declarator declarator: (_) @decl))) (field_declaration (_) @type (init_declarator declarator: (_) @decl)) (field_declaration (_) @type (_) @decl) (declaration (_) @type (init_declarator declarator: (_) @decl))";
    let query = Query::new(&language, query_str)?;
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, *root, content.as_bytes());
    let mut best_type = None;
    let mut best_row = 0;
    while let Some(m) = matches.next() {
        let mut type_node: Option<Node> = None;
        let mut decl_nodes: Vec<Node> = Vec::new();
        for cap in m.captures {
            let c_name = query.capture_names()[cap.index as usize];
            if c_name == "type" { type_node = Some(cap.node); }
            else if c_name == "decl" { decl_nodes.push(cap.node); }
        }
        for d_node in decl_nodes {
            if find_identifier_in_decl(&d_node, target_name, content)? {
                let row = d_node.start_position().row;
                if row <= cursor_row && (best_type.is_none() || row >= best_row) {
                    if let Some(t_node) = type_node {
                        let type_text = get_node_text(&t_node, content).trim();
                        if type_text == "auto" {
                            if let Some(inferred) = infer_from_assignment(ctx, target_name, root, content, cursor_row)? {
                                best_type = Some(inferred);
                            } else if let Some(range_type) = infer_for_range_element_type(ctx, d_node, root, content, cursor_row)? {
                                best_type = Some(range_type);
                            }
                        } else {
                            best_type = Some(extract_clean_type(type_text));
                        }
                    } else {
                        // type_node なし = for_range_loop パターン（declarator: のみキャプチャ）
                        if let Some(range_type) = infer_for_range_element_type(ctx, d_node, root, content, cursor_row)? {
                            best_type = Some(range_type);
                        }
                    }
                    best_row = row;
                }
            }
        }
    }
    if best_type.is_none() { best_type = infer_from_assignment(ctx, target_name, root, content, cursor_row)?; }
    Ok(best_type)
}

/// for_range_loop の宣言ノードから上に辿り、イテラブルの要素型を推論する。
/// `for (auto component : mMeshArray)` → mMeshArray の型を解決し、コンテナを unwrap して返す。
/// grammar.js 上では `_for_range_loop_body` の `right:` フィールドがイテラブルに対応。
fn infer_for_range_element_type(
    ctx: &mut RequestContext,
    decl_node: Node,
    root: &Node,
    content: &str,
    cursor_row: usize,
) -> anyhow::Result<Option<String>> {
    let mut current = decl_node;
    loop {
        let parent = match current.parent() {
            Some(p) => p,
            None => return Ok(None),
        };
        if parent.kind() == "for_range_loop" {
            // _for_range_loop_body の right: フィールドがイテラブル
            if let Some(node) = parent.child_by_field_name("right") {
                if let Ok(Some(iterable_type)) = resolve_expression_type(ctx, node, root, content, cursor_row) {
                    return Ok(Some(unwrap_container_type(&iterable_type)));
                }
            }
            // フォールバック: ':' の後ろの最初の named ノードを探す
            let mut found_colon = false;
            for i in 0..parent.child_count() {
                if let Some(child) = parent.child(i as u32) {
                    if found_colon && child.is_named() && child.kind() != "compound_statement" {
                        if let Ok(Some(iterable_type)) = resolve_expression_type(ctx, child, root, content, cursor_row) {
                            return Ok(Some(unwrap_container_type(&iterable_type)));
                        }
                        return Ok(None);
                    }
                    if child.kind() == ":" { found_colon = true; }
                }
            }
            return Ok(None);
        }
        current = parent;
    }
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
            if ["NewObject", "TObjectPtr", "TSharedPtr", "StaticCastSharedPtr", "MakeShared", "MakeUnique"].contains(&func) { return Ok(Some(extract_clean_type(inner))); }
            return Ok(Some(extract_clean_type(func)));
        }
    }
    if let Ok(re) = regex::Regex::new(r"^([a-zA-Z0-9_:]+)\s*\(") {
        if let Some(cap) = re.captures(text) {
            let full_name = cap.get(1).unwrap().as_str();
            if full_name.contains("::") {
                let parts: Vec<&str> = full_name.split("::").collect();
                if let Some(&last) = parts.last() {
                    if ["Get", "GetChecked", "GetPtr", "StaticClass", "GetDefault", "GetInstance"].contains(&last) {
                        return Ok(Some(extract_clean_type(&parts[..parts.len()-1].join("::"))));
                    }
                }
            }
            return Ok(Some(extract_clean_type(full_name)));
        }
    }
    Ok(None)
}

fn extract_clean_type(raw: &str) -> String {
    let mut clean = raw.trim().to_string();
    
    // 1. 不要なキーワードの削除
    // 単語全体 (\b) に厳密にマッチさせる
    let keywords = ["const", "typename", "struct", "class", "enum", "virtual", "static", "inline", "FORCEINLINE", "volatile", "mutable"];
    for kw in keywords {
        let pattern = format!(r"\b{}\b", kw);
        if let Ok(re) = regex::Regex::new(&pattern) {
            clean = re.replace_all(&clean, " ").to_string();
        }
    }
    
    // 2. UE特有のマクロ削除 (_API, UPROPERTY 等)
    // マクロ名そのものにマッチさせる (\b..._API\b)
    if let Ok(re) = regex::Regex::new(r"\b[A-Z0-9_]+_API\b") {
        clean = re.replace_all(&clean, " ").to_string();
    }
    // UPROPERTY や UFUNCTION などの単体キーワードも削る
    let ue_keywords = ["UPROPERTY", "UFUNCTION", "UCLASS", "USTRUCT", "UENUM", "GENERATED_BODY"];
    for kw in ue_keywords {
        let pattern = format!(r"\b{}\b", kw);
        if let Ok(re) = regex::Regex::new(&pattern) {
            clean = re.replace_all(&clean, " ").to_string();
        }
    }

    // 3. テンプレートの抽出
    if let Some(start) = clean.find('<') {
        if let Some(end) = clean.rfind('>') {
            let wrapper = clean[..start].trim();
            let inner = &clean[start+1..end];
            if ["TObjectPtr", "TSharedPtr", "TUniquePtr", "TWeakObjectPtr", "TSubclassOf", "TSoftObjectPtr", "TSoftClassPtr", "TEnumAsByte"].contains(&wrapper) {
                return extract_clean_type(inner);
            }
            // テンプレートを維持する場合
            return format!("{}<{}>", wrapper.replace(['*', '&'], "").trim(), inner).trim().to_string();
        }
    }

    // 4. 装飾子の除去
    // * と & を消した後、トリムする。
    // その後、もし末尾に > が残っていたら除去する（DBの不整合対策の最終ガード）
    let mut res = clean.replace(['*', '&'], "").trim().to_string();
    if res.ends_with('>') && !res.contains('<') {
        res.pop();
    }
    res.trim().to_string()
}

/// ファイルのフルパス文字列からDBのfile_idを取得する。
/// ファイル名だけでなくディレクトリ階層まで照合するため、同名ファイルが複数存在しても正しいIDを返す。
fn get_file_id_by_full_path(conn: &Connection, file_path: &str) -> Option<i64> {
    let path = std::path::Path::new(file_path);
    let filename = path.file_name()?.to_str()?;
    let parent = path.parent()?;

    let mut current_parent_id: Option<i64> = None;
    for component in parent.components() {
        let name = match component {
            std::path::Component::Normal(s) => s.to_str()?,
            std::path::Component::RootDir => "/",
            std::path::Component::Prefix(p) => p.as_os_str().to_str()?,
            _ => continue,
        };

        let dir_id: Option<i64> = conn.query_row(
            "SELECT d.id FROM directories d JOIN strings s ON d.name_id = s.id
             WHERE (d.parent_id IS ? OR d.parent_id = ?) AND s.text = ?",
            rusqlite::params![current_parent_id, current_parent_id, name],
            |r| r.get(0),
        ).optional().ok().flatten();

        match dir_id {
            Some(id) => current_parent_id = Some(id),
            None => return None,
        }
    }

    let dir_id = current_parent_id?;
    conn.query_row(
        "SELECT f.id FROM files f JOIN strings s ON f.filename_id = s.id
         WHERE f.directory_id = ? AND s.text = ?",
        rusqlite::params![dir_id, filename],
        |r| r.get(0),
    ).optional().ok().flatten()
}
