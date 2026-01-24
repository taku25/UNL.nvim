use std::io::{self, Read, Write};
use std::fs;
use std::sync::{Arc, Mutex};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tree_sitter::{Parser, Query, QueryCursor, Node};
use streaming_iterator::StreamingIterator;
use sha2::{Sha256, Digest};

#[derive(Deserialize)]
struct InputFile {
    path: String,
    mtime: u64,
    old_hash: Option<String>,
}

#[derive(Serialize)]
struct ParseResult {
    path: String,
    status: String,
    mtime: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<ParseData>,
}

#[derive(Serialize)]
struct ParseData {
    classes: Vec<ClassInfo>,
    parser: String,
    new_hash: String,
}

#[derive(Serialize, Clone)]
struct ClassInfo {
    class_name: String,
    base_class: Option<String>,
    symbol_type: String,
    line: usize,
    #[serde(skip)]
    range_start: usize,
    #[serde(skip)]
    range_end: usize,
    members: Vec<MemberInfo>,
    is_final: bool,
    is_interface: bool,
}

#[derive(Serialize, Clone)]
struct MemberInfo {
    name: String,
    #[serde(rename = "type")]
    mem_type: String,
    flags: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

const QUERY_STR: &str = r#"
  (class_specifier name: (_) @class_name) @class_def
  (struct_specifier name: (_) @struct_name) @struct_def
  (enum_specifier name: (_) @enum_name) @enum_def
  
  (unreal_class_declaration name: (_) @class_name) @uclass_def
  (unreal_struct_declaration name: (_) @struct_name) @ustruct_def
  (unreal_enum_declaration name: (_) @enum_name) @uenum_def
  
  (unreal_declare_class_macro) @declare_class_macro

  ;; Base Class extraction
  (base_class_clause
    (access_specifier)?
    (type_identifier) @base_class_name
  )

  ;; Improved Function Captures
  (function_definition
    declarator: [
      (function_declarator declarator: (_) @func_name)
      (pointer_declarator (_) @func_name)
      (reference_declarator (_) @func_name)
    ]
  ) @func_node

  (declaration
    declarator: [
      (function_declarator declarator: (_) @func_name)
      (pointer_declarator (_) @func_name)
      (reference_declarator (_) @func_name)
      (field_identifier) @prop_name
      (identifier) @prop_name
    ]
  ) @decl_node

  (unreal_function_declaration
    declarator: (_) @func_name
  ) @ufunc_node

  ;; Generic Field Captures
  (field_declaration
    declarator: [
      (field_identifier) @prop_name
      (pointer_declarator (_) @prop_name)
      (reference_declarator (_) @prop_name)
      (array_declarator (_) @prop_name)
    ]
  ) @field_node
"#;

fn main() -> anyhow::Result<()> {
    let language = tree_sitter_unreal_cpp::LANGUAGE.into();
    let query = Arc::new(Query::new(&language, QUERY_STR).expect("Failed to parse query"));

    let args: Vec<String> = std::env::args().collect();
    let buffer = if args.len() > 1 {
        fs::read_to_string(&args[1])?
    } else {
        let mut b = String::new();
        io::stdin().read_to_string(&mut b)?;
        b
    };
    
    if buffer.trim().is_empty() { return Ok(()); }

    let inputs: Vec<InputFile> = if buffer.trim().starts_with('[') {
        serde_json::from_str(&buffer)?
    } else {
        vec![serde_json::from_str(&buffer)?]
    };

    let stdout_mutex = Arc::new(Mutex::new(io::stdout()));

    inputs.into_par_iter().for_each(|input| {
        let result = process_file(&input, &language, &query);
        
        if let Ok(res) = result {
            if let Ok(json) = serde_json::to_string(&res) {
                let mut out = stdout_mutex.lock().unwrap();
                writeln!(out, "{}", json).ok();
            }
        } else {
            eprintln!("Error processing {}: {:?}", input.path, result.err());
        }
    });

    Ok(())
}

fn get_node_text<'a>(node: &Node, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

fn find_child_by_type<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == kind { return Some(child); }
        if let Some(found) = find_child_by_type(child, kind) { return Some(found); }
    }
    None
}

fn has_child_type(node: Node, type_name: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == type_name { return true; }
    }
    false
}

fn process_file(input: &InputFile, language: &tree_sitter::Language, query: &Query) -> anyhow::Result<ParseResult> {
    let content = fs::read_to_string(&input.path)?;
    let content_bytes = content.as_bytes();
    
    let mut hasher = Sha256::new();
    hasher.update(content_bytes);
    let new_hash = format!("{:x}", hasher.finalize());

    if let Some(old) = &input.old_hash {
        if old == &new_hash {
            return Ok(ParseResult {
                path: input.path.clone(), status: "cache_hit".to_string(), mtime: input.mtime, data: None,
            });
        }
    }

    let mut parser = Parser::new();
    parser.set_language(language).unwrap();
    
    let tree = parser.parse(&content, None).ok_or(anyhow::anyhow!("Parse failed"))?;
    let root = tree.root_node();
    
    let mut cursor = QueryCursor::new();
    let mut captures = cursor.captures(query, root, content_bytes);
    
    let mut classes: Vec<ClassInfo> = Vec::new();
    let mut members: Vec<(MemberInfo, usize, usize)> = Vec::new();

    while let Some((m, capture_index)) = captures.next() {
        let capture = m.captures[*capture_index];
        let capture_name: &str = &query.capture_names()[capture.index as usize];
        let node = capture.node;
        
        if capture_name == "class_name" || capture_name == "struct_name" || capture_name == "enum_name" {
            if let Some(parent) = node.parent() {
                if parent.child_by_field_name("body").is_none() { continue; }

                let name = get_node_text(&node, content_bytes).to_string();
                let mut symbol_type = "class";
                if capture_name == "struct_name" { symbol_type = "struct"; }
                if capture_name == "enum_name" { symbol_type = "enum"; }
                
                let kind_str = parent.kind();
                if kind_str == "unreal_class_declaration" { symbol_type = "UCLASS"; }
                else if kind_str == "unreal_struct_declaration" { symbol_type = "USTRUCT"; }
                else if kind_str == "unreal_enum_declaration" { symbol_type = "UENUM"; }

                classes.push(ClassInfo {
                    class_name: name,
                    base_class: None,
                    symbol_type: symbol_type.to_string(),
                    line: node.start_position().row + 1,
                    range_start: parent.start_byte(),
                    range_end: parent.end_byte(),
                    members: Vec::new(),
                    is_final: false,
                    is_interface: false,
                });
            }
        } else if capture_name == "base_class_name" {
            if let Some(cls) = classes.last_mut() {
                cls.base_class = Some(get_node_text(&node, content_bytes).to_string());
            }
        } else if capture_name == "func_name" || capture_name == "prop_name" {
            let member_name_raw = get_node_text(&node, content_bytes);
            
            let mut definition_node = node;
            for _ in 0..5 {
                let kind = definition_node.kind();
                if kind.contains("declaration") || kind.contains("definition") { break; }
                if let Some(p) = definition_node.parent() { definition_node = p; } else { break; }
            }
            
            let mut flags = Vec::new();
            if has_child_type(definition_node, "ufunction_macro") || has_child_type(definition_node, "unreal_function_macro") || definition_node.kind() == "unreal_function_declaration" {
                flags.push("UFUNCTION");
            }
            if has_child_type(definition_node, "uproperty_macro") || has_child_type(definition_node, "unreal_property_macro") {
                flags.push("UPROPERTY");
            }
            
            let node_text = get_node_text(&definition_node, content_bytes);
            if node_text.contains("virtual") { flags.push("virtual"); }
            if node_text.contains("static") { flags.push("static"); }
            if node_text.contains("override") { flags.push("override"); }

            let mut detail = None;
            let mem_type = if capture_name == "func_name" { "function" } else { "property" };

            if mem_type == "function" {
                if let Some(param_list) = find_child_by_type(definition_node, "parameter_list") {
                    detail = Some(get_node_text(&param_list, content_bytes).to_string());
                }
            }

            let mut name = member_name_raw.split(|c| c == '(' || c == '[' || c == '=' || c == ';').next().unwrap_or("").trim();
            name = name.trim_start_matches(|c| c == '*' || c == '&' || c == ' ').trim();
            name = name.split_whitespace().last().unwrap_or(name);

            if !name.is_empty() && name != "virtual" && name != "static" && name != "void" && name != "const" {
                members.push((MemberInfo {
                    name: name.to_string(),
                    mem_type: mem_type.to_string(),
                    flags: flags.join(" "),
                    detail,
                }, definition_node.start_byte(), definition_node.end_byte()));
            }
        }
    }
    
    for (member, m_start, m_end) in members {
        let mut best_class_idx = None;
        let mut min_size = usize::MAX;
        for (i, cls) in classes.iter().enumerate() {
            if m_start >= cls.range_start && m_end <= cls.range_end {
                let size = cls.range_end - cls.range_start;
                if size < min_size { min_size = size; best_class_idx = Some(i); }
            }
        }
        if let Some(idx) = best_class_idx { classes[idx].members.push(member); }
    }

    Ok(ParseResult {
        path: input.path.clone(), status: "parsed".to_string(), mtime: input.mtime,
        data: Some(ParseData { classes, parser: "treesitter".to_string(), new_hash }),
    })
}