use serde_json::{json, Value};
use tree_sitter::{Node, Parser, Query, QueryCursor, Point, StreamingIterator};

pub fn parse_buffer(
    content: String, 
    file_path: Option<String>,
    line: Option<u32>,
    character: Option<u32>
) -> anyhow::Result<Value> {
    let path = file_path.map(|p| if std::path::MAIN_SEPARATOR == '\\' { p.replace('\\', "/") } else { p }).unwrap_or_else(|| "buffer.cpp".to_string());
    let language: tree_sitter::Language = tree_sitter_unreal_cpp::LANGUAGE.into();
    
    // 1. 全体シンボル解析 (既存の scanner ロジックを利用)
    let query = Query::new(&language, crate::scanner::QUERY_STR)?;
    let classes = crate::scanner::parse_content(&content, &path, &language, &query)?;
    
    let mut results = Vec::new();
    for cls in classes {
        let mut class_info = json!({ 
            "name": cls.class_name, 
            "kind": cls.symbol_type, 
            "line": cls.line, 
            "end_line": cls.end_line, 
            "namespace": cls.namespace, 
            "base_class": cls.base_classes.first(), 
            "file_path": path, 
            "fields": { "public": [], "protected": [], "private": [] }, 
            "methods": { "public": [], "protected": [], "private": [] } 
        });
        for m in cls.members {
            let access = m.access.to_lowercase();
            let m_json = json!({ 
                "name": m.name, "kind": m.mem_type, "flags": m.flags, "access": m.access, 
                "detail": m.detail, "return_type": m.return_type, "file_path": path, 
                "line": m.line, "end_line": m.end_line 
            });
            let target = if m.mem_type.to_lowercase().contains("function") { "methods" } else { "fields" };
            class_info[target].as_object_mut().unwrap().entry(access).or_insert(json!([])).as_array_mut().unwrap().push(m_json);
        }
        results.push(class_info);
    }

    let mut parser = Parser::new();
    parser.set_language(&language)?;
    let tree = parser.parse(&content, None).ok_or_else(|| anyhow::anyhow!("Failed to parse buffer"))?;
    let root = tree.root_node();
    
    // 2. インクルード解析
    let mut generated_h_line = 0;
    let mut last_include_line = 0;
    let mut include_regions = Vec::new();

    let include_query_str = "(preproc_include path: [(string_literal) @path (system_lib_string) @path]) @include";
    let include_query = Query::new(&language, include_query_str)?;
    let mut include_cursor = QueryCursor::new();
    let mut include_matches = include_cursor.matches(&include_query, root, content.as_bytes());

    while let Some(m) = include_matches.next() {
        let mut path_text = String::new();
        let mut full_node = None;
        for cap in m.captures {
            let name = include_query.capture_names()[cap.index as usize];
            if name == "path" { 
                path_text = get_node_text(&cap.node, &content).trim_matches('"').trim_matches('<').trim_matches('>').to_string(); 
            }
            else if name == "include" { full_node = Some(cap.node); }
        }
        if let Some(node) = full_node {
            let start_line = node.start_position().row + 1;
            last_include_line = start_line;
            let is_generated = path_text.contains(".generated.h");
            if is_generated { generated_h_line = start_line; }
            include_regions.push(json!({ "line": start_line, "path": path_text, "is_generated": is_generated }));
        }
    }

    let mut suggested_line = if last_include_line > 0 { last_include_line + 1 } else { 1 };
    if generated_h_line > 0 && suggested_line > generated_h_line { suggested_line = generated_h_line; }
    if last_include_line == 0 {
        for (i, l) in content.lines().enumerate() {
            if l.contains("#pragma once") { suggested_line = (i + 2) as usize; break; }
        }
    }

    // 3. カーソル位置のノード解析 (UCM copy_imp 等のため)
    let mut cursor_info = Value::Null;
    if let (Some(l), Some(c)) = (line, character) {
        let point = Point::new(l as usize, c as usize);
        if let Some(node) = root.descendant_for_point_range(point, point) {
            cursor_info = analyze_cursor_node(node, &content);
        }
    }

    Ok(json!({
        "symbols": results,
        "cursor_info": cursor_info,
        "metadata": {
            "generated_h_line": generated_h_line,
            "last_include_line": last_include_line,
            "suggested_insert_line": suggested_line,
            "includes": include_regions,
        }
    }))
}

fn analyze_cursor_node(node: Node, content: &str) -> Value {
    let mut curr = Some(node);
    while let Some(n) = curr {
        let kind = n.kind();
        // 関数宣言（または定義）を探す
        if kind == "field_declaration" || kind == "function_definition" || kind == "unreal_function_declaration" {
            // シグネチャ情報の抽出
            let mut name = "";
            let mut return_type = "";
            let mut params = "";
            let mut is_virtual = false;
            let mut is_static = false;
            let mut is_const = false;

            let text = get_node_text(&n, content);
            if text.contains("virtual") { is_virtual = true; }
            if text.contains("static") { is_static = true; }
            if text.contains("const") && !text.contains("const ") { // Simple heuristic
                 is_const = true;
            }

            // declarator を探して名前と引数を取得
            if let Some(decl) = find_child_by_field(&n, "declarator") {
                if let Some(name_node) = find_name_node(decl) {
                    name = get_node_text(&name_node, content);
                }
                if let Some(params_node) = find_child_by_type(decl, "parameter_list") {
                    params = get_node_text(&params_node, content);
                }
            }

            // 戻り値の型
            if let Some(type_node) = find_child_by_field(&n, "type") {
                return_type = get_node_text(&type_node, content);
            }

            // 所属クラスの特定
            let class_name = find_enclosing_class_name(n, content).unwrap_or_default();

            return json!({
                "kind": kind,
                "name": name,
                "class_name": class_name,
                "return_type": return_type,
                "parameters": params,
                "is_virtual": is_virtual,
                "is_static": is_static,
                "is_const": is_const,
                "full_text": text,
            });
        }
        curr = n.parent();
    }
    Value::Null
}

fn find_name_node(node: Node) -> Option<Node> {
    match node.kind() {
        "identifier" | "field_identifier" => Some(node),
        "function_declarator" | "pointer_declarator" | "reference_declarator" => {
            if let Some(child) = node.child_by_field_name("declarator") {
                find_name_node(child)
            } else { None }
        }
        _ => {
            for i in 0..node.child_count() {
                if let Some(res) = find_name_node(node.child(i as u32).unwrap()) { return Some(res); }
            }
            None
        }
    }
}

fn find_child_by_field<'a>(node: &Node<'a>, field: &str) -> Option<Node<'a>> {
    node.child_by_field_name(field)
}

fn find_child_by_type<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == kind { return Some(child); }
        if let Some(found) = find_child_by_type(child, kind) { return Some(found); }
    }
    None
}

fn find_enclosing_class_name(node: Node, content: &str) -> Option<String> {
    let mut curr = node.parent();
    while let Some(n) = curr {
        let kind = n.kind();
        if kind == "class_specifier" || kind == "struct_specifier" || kind == "unreal_class_declaration" || kind == "unreal_struct_declaration" {
            if let Some(name_node) = n.child_by_field_name("name") {
                return Some(get_node_text(&name_node, content).trim().to_string());
            }
        }
        curr = n.parent();
    }
    None
}

fn get_node_text<'a>(node: &Node, content: &'a str) -> &'a str {
    let range = node.byte_range();
    if range.end <= content.len() {
        &content[range.start..range.end]
    } else {
        ""
    }
}