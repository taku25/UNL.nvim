use serde_json::{json, Value};
use tree_sitter::{Node, Parser, Query, QueryCursor, StreamingIterator};

pub fn parse_buffer(content: String, file_path: Option<String>) -> anyhow::Result<Value> {
    let path = file_path.map(|p| if std::path::MAIN_SEPARATOR == '\\' { p.replace('\\', "/") } else { p }).unwrap_or_else(|| "buffer.cpp".to_string());
    let language: tree_sitter::Language = tree_sitter_unreal_cpp::LANGUAGE.into();
    let query = Query::new(&language, crate::scanner::QUERY_STR)?;
    let classes = crate::scanner::parse_content(&content, &path, &language, &query)?;
    
    let mut results = Vec::new();
    for cls in classes {
        let mut class_info = json!({ "name": cls.class_name, "kind": cls.symbol_type, "line": cls.line, "end_line": cls.end_line, "namespace": cls.namespace, "base_class": cls.base_classes.first(), "file_path": path, "fields": { "public": [], "protected": [], "private": [] }, "methods": { "public": [], "protected": [], "private": [] } });
        for m in cls.members {
            let access = m.access.to_lowercase();
            let m_json = json!({ "name": m.name, "kind": m.mem_type, "flags": m.flags, "access": m.access, "detail": m.detail, "return_type": m.return_type, "file_path": path, "line": m.line, "end_line": m.end_line });
            let target = if m.mem_type.to_lowercase().contains("function") { "methods" } else { "fields" };
            class_info[target].as_object_mut().unwrap().entry(access).or_insert(json!([])).as_array_mut().unwrap().push(m_json);
        }
        results.push(class_info);
    }

    let mut parser = Parser::new();
    parser.set_language(&language)?;
    let tree = parser.parse(&content, None).unwrap();
    let root = tree.root_node();
    
    let mut generated_h_line = 0;
    let mut last_include_line = 0;
    let mut include_regions = Vec::new();

    let include_query_str = "(preproc_include path: [(string_literal) @path (system_lib_string) @path]) @include";
    let include_query = Query::new(&language, include_query_str)?;
    let mut include_cursor = QueryCursor::new();
    let mut matches = include_cursor.matches(&include_query, root, content.as_bytes());

    while let Some(m) = matches.next() {
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
        for (i, line) in content.lines().enumerate() {
            if line.contains("#pragma once") { suggested_line = (i + 2) as usize; break; }
        }
    }

    Ok(json!({
        "symbols": results,
        "metadata": {
            "generated_h_line": generated_h_line,
            "last_include_line": last_include_line,
            "suggested_insert_line": suggested_line,
            "includes": include_regions,
        }
    }))
}

fn get_node_text<'a>(node: &Node, content: &'a str) -> &'a str {
    let range = node.byte_range();
    if range.end <= content.len() {
        &content[range.start..range.end]
    } else {
        ""
    }
}
