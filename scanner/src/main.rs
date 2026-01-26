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
    namespace: Option<String>,
    base_classes: Vec<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    return_type: Option<String>,
}

const QUERY_STR: &str = r#"
  (class_specifier name: (_) @class_name) @class_def
  (struct_specifier name: (_) @struct_name) @struct_def
  (enum_specifier name: (_) @enum_name) @enum_def
  
  ;; Fallback: capture type_identifier directly inside struct/class specifier
  (struct_specifier (type_identifier) @struct_name)
  (class_specifier (type_identifier) @class_name)

  (unreal_class_declaration name: (_) @class_name) @uclass_def
  (unreal_struct_declaration name: (_) @struct_name) @ustruct_def
  (unreal_enum_declaration name: (_) @enum_name) @uenum_def
  
  (unreal_declare_class_macro) @declare_class_macro

  ;; Alias Declaration (using FTransform = ...)
  (alias_declaration) @alias_decl

  ;; Type Definition (typedef Old New;)
  (type_definition) @typedef_decl

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
      (function_declarator declarator: (_) @func_name)
      (pointer_declarator (function_declarator declarator: (_) @func_name))
      (reference_declarator (function_declarator declarator: (_) @func_name))
    ]
  ) @field_node

  ;; Enum values
  (enumerator
    name: (identifier) @enum_val_name
  ) @enum_item
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
        }
    });

    Ok(())
}

fn get_node_text<'a>(node: &Node, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

fn get_namespace<'a>(node: &Node<'a>, source: &'a [u8]) -> Option<String> {
    let mut ns_parts = Vec::new();
    let mut current = node.parent();
    
    while let Some(n) = current {
        if n.kind() == "namespace_definition" {
            // Check for direct name field or nested names (namespace UE::Math)
            if let Some(name_node) = n.child_by_field_name("name") {
                ns_parts.push(get_node_text(&name_node, source).to_string());
            }
        }
        current = n.parent();
    }
    
    if ns_parts.is_empty() {
        None
    } else {
        ns_parts.reverse();
        Some(ns_parts.join("::"))
    }
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

fn clean_type_string(s: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    for word in s.split_whitespace() {
        let w = word.trim();
        if w.is_empty() { continue; }
        
        if w == "virtual" || w == "static" || w == "inline" || w == "FORCEINLINE" || 
           w == "const" || w == "friend" || w == "class" || w == "struct" || w == "enum" ||
           w.ends_with("_API") { 
            continue;
        }
        
        if w.starts_with("UFUNCTION") || w.starts_with("UPROPERTY") {
            continue;
        }
        
        words.push(w.to_string());
    }
    words.join(" ")
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
            let has_body = if let Some(parent) = node.parent() {
                parent.child_by_field_name("body").is_some()
            } else { false };

            if has_body {
                if let Some(parent) = node.parent() {
                    let name = get_node_text(&node, content_bytes).to_string();
                    let namespace = get_namespace(&parent, content_bytes);
                    
                    let mut symbol_type = "class";
                    if capture_name == "struct_name" { symbol_type = "struct"; }
                    if capture_name == "enum_name" { symbol_type = "enum"; }
                    
                    let kind_str = parent.kind();
                    if kind_str == "unreal_class_declaration" { symbol_type = "UCLASS"; }
                    else if kind_str == "unreal_struct_declaration" { symbol_type = "USTRUCT"; }
                    else if kind_str == "unreal_enum_declaration" { symbol_type = "UENUM"; }

                    let range_start = parent.start_byte();
                    let range_end = parent.end_byte();
                    
                    // Avoid duplicates (overlapping captures)
                    let mut exists = false;
                    for c in &classes {
                        if c.range_start == range_start && c.range_end == range_end {
                            exists = true;
                            break;
                        }
                    }

                    if !exists {
                        classes.push(ClassInfo {
                            class_name: name,
                            namespace,
                            base_classes: Vec::new(),
                            symbol_type: symbol_type.to_string(),
                            line: node.start_position().row + 1,
                            range_start,
                            range_end,
                            members: Vec::new(),
                            is_final: false,
                            is_interface: false,
                        });
                    }
                }
            }
        } else if capture_name == "alias_decl" {
            // Manual extraction for alias_declaration
            if let Some(name_node) = node.child_by_field_name("name") {
                 let name = get_node_text(&name_node, content_bytes).to_string();
                 let namespace = get_namespace(&node, content_bytes);
                 
                 if let Some(type_node) = node.child_by_field_name("type") {
                     let mut target_type = get_node_text(&type_node, content_bytes).to_string();
                     if let Some(idx) = target_type.find('<') { target_type = target_type[..idx].to_string(); }
                     if let Some(idx) = target_type.rfind("::") { target_type = target_type[idx+2..].to_string(); }
                     target_type = target_type.trim().to_string();
                     
                     if !name.is_empty() && !target_type.is_empty() {
                         classes.push(ClassInfo {
                            class_name: name, namespace, base_classes: vec![target_type], symbol_type: "struct".to_string(),
                            line: node.start_position().row + 1, range_start: node.start_byte(), range_end: node.end_byte(),
                            members: Vec::new(), is_final: false, is_interface: false,
                         });
                     }
                 }
            }

        } else if capture_name == "typedef_decl" {
            // Manual extraction for type_definition (typedef T A;)
            if let Some(name_node) = node.child_by_field_name("declarator") {
                 let name = get_node_text(&name_node, content_bytes).to_string();
                 
                 // Filter out function pointers, templates, pointers, refs, consts, nested types in the name itself.
                 // We only want to capture simple aliases like "typedef TVector<double> FVector3d;" 
                 // where the name "FVector3d" is a clean identifier.
                 if name.contains('(') || name.contains(')') || name.contains('<') || name.contains('>') ||
                    name.contains(':') || name.contains('*') || name.contains('&') || 
                    name.contains("const") || name.contains(' ') {
                     // Skip complex declarators (function pointers, etc.)
                 } else {
                     let namespace = get_namespace(&node, content_bytes);
                     
                     if let Some(type_node) = node.child_by_field_name("type") {
                         let mut target_type = get_node_text(&type_node, content_bytes).to_string();
                         if let Some(idx) = target_type.find('<') { target_type = target_type[..idx].to_string(); }
                         if let Some(idx) = target_type.rfind("::") { target_type = target_type[idx+2..].to_string(); }
                         target_type = target_type.trim().to_string();
                         
                         if !name.is_empty() && !target_type.is_empty() {
                             classes.push(ClassInfo {
                                class_name: name, namespace, base_classes: vec![target_type], symbol_type: "struct".to_string(),
                                line: node.start_position().row + 1, range_start: node.start_byte(), range_end: node.end_byte(),
                                members: Vec::new(), is_final: false, is_interface: false,
                             });
                         }
                     }
                 }
            }

        } else if capture_name == "base_class_name" {
            if let Some(cls) = classes.last_mut() {
                let mut name = get_node_text(&node, content_bytes).to_string();
                if let Some(idx) = name.rfind("::") {
                    name = name[idx+2..].to_string();
                }
                if name != cls.class_name {
                    cls.base_classes.push(name);
                }
            }
        } else if capture_name == "func_name" || capture_name == "prop_name" {
            let member_name_raw = get_node_text(&node, content_bytes);
            let mut definition_node = node;
            let mut ufunc_wrapper = None;

            for _ in 0..6 {
                let kind = definition_node.kind();
                if kind == "unreal_function_declaration" {
                    ufunc_wrapper = Some(definition_node);
                    break; 
                }
                if kind.contains("declaration") || kind.contains("definition") { break; }
                if let Some(p) = definition_node.parent() { definition_node = p; } else { break; }
            }
            
            if let Some(wrapper) = ufunc_wrapper {
                definition_node = wrapper;
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
            let mut return_type = None;
            let mem_type = if capture_name == "func_name" { "function" } else { "property" };

            let func_name_clean = member_name_raw.split(|c| c == '(' || c == '[' || c == '=' || c == ';').next().unwrap_or("").trim();
            let def_text = get_node_text(&definition_node, content_bytes);
            
            if let Some(idx) = def_text.find(func_name_clean) {
                let prefix = &def_text[..idx];
                let mut actual_prefix = prefix;
                if let Some(macro_end) = prefix.rfind(')') {
                    actual_prefix = &prefix[macro_end+1..];
                }
                let cleaned = clean_type_string(actual_prefix);
                if !cleaned.is_empty() { return_type = Some(cleaned); }
            }

            if mem_type == "function" {
                if let Some(param_list) = find_child_by_type(definition_node, "parameter_list") {
                    detail = Some(get_node_text(&param_list, content_bytes).to_string());
                }
            }

            let name_tmp = func_name_clean.trim_start_matches(|c| c == '*' || c == '&' || c == ' ').trim();
            let clean_name = name_tmp.split_whitespace().last().unwrap_or(name_tmp);

            if !clean_name.is_empty() && clean_name != "virtual" && clean_name != "static" && clean_name != "void" && clean_name != "const" {
                members.push((MemberInfo {
                    name: clean_name.to_string(),
                    mem_type: mem_type.to_string(),
                    flags: flags.join(" "),
                    detail,
                    return_type,
                }, definition_node.start_byte(), definition_node.end_byte()));
            }
        } else if capture_name == "enum_val_name" {
            let name = get_node_text(&node, content_bytes).to_string();
            if !name.is_empty() {
                members.push((MemberInfo {
                    name,
                    mem_type: "enum_item".to_string(),
                    flags: String::new(),
                    detail: None,
                    return_type: None,
                }, node.start_byte(), node.end_byte()));
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