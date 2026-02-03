use std::fs;
use tree_sitter::{Parser, Query, QueryCursor, Node};
use streaming_iterator::StreamingIterator;
use sha2::{Sha256, Digest};
use crate::types::{InputFile, ParseResult, ParseData, ClassInfo, MemberInfo};

pub const QUERY_STR: &str = r#"
  (class_specifier name: (type_identifier) @class_name) @class_def
  (struct_specifier name: (type_identifier) @struct_name) @struct_def
  (enum_specifier name: (type_identifier) @enum_name) @enum_def
  (struct_specifier (type_identifier) @struct_name)
  (class_specifier (type_identifier) @class_name)
  (enum_specifier (type_identifier) @enum_name)
  (unreal_class_declaration name: (type_identifier) @class_name) @uclass_def
  (unreal_struct_declaration name: (_) @struct_name) @ustruct_def
  (unreal_enum_declaration name: (_) @enum_name) @uenum_def
  (unreal_declaration_macro
    name: (unreal_macro_name) @macro_type
    arguments: (unreal_argument_list
      (unreal_specifier_list
        (unreal_specifier
          (unreal_specifier_content
            (identifier) @macro_item_name))))
  ) @unreal_macro
  (alias_declaration) @alias_decl
  (type_definition) @typedef_decl
  (base_class_clause (access_specifier)? (type_identifier) @base_class_name)
  
  (function_definition) @func_node
  (declaration) @decl_node
  (unreal_function_declaration) @ufunc_node
  (field_declaration) @field_node
  (enumerator name: (identifier) @enum_val_name) @enum_item
"#;

pub fn process_file(input: &InputFile, language: &tree_sitter::Language, query: &Query) -> anyhow::Result<ParseResult> {
    let content = fs::read_to_string(&input.path)?;
    let content_bytes = content.as_bytes();
    
    let mut hasher = Sha256::new();
    hasher.update(content_bytes);
    let new_hash = format!("{:x}", hasher.finalize());

    if let Some(old) = &input.old_hash {
        if old == &new_hash {
            return Ok(ParseResult {
                path: input.path.clone(),
                status: "cache_hit".to_string(),
                mtime: input.mtime,
                data: None,
                module_id: input.module_id,
            });
        }
    }

    let mut parser = Parser::new();
    parser.set_language(&language).unwrap();
    let tree = parser.parse(&content, None).ok_or(anyhow::anyhow!("Parse failed"))?;
    let root = tree.root_node();
    
    let mut cursor = QueryCursor::new();
    let mut captures = cursor.captures(query, root, content_bytes);
    
    let mut classes: Vec<ClassInfo> = Vec::new();
    let mut members: Vec<(MemberInfo, usize, usize)> = Vec::new();

    while let Some((m, capture_index)) = captures.next() {
        let capture = m.captures[*capture_index];
        let capture_name = &query.capture_names()[capture.index as usize];
        let node = capture.node;
        
        if *capture_name == "class_name" || *capture_name == "struct_name" || *capture_name == "enum_name" {
            let has_body = if let Some(parent) = node.parent() {
                parent.child_by_field_name("body").is_some()
            } else { false };

            if has_body {
                if let Some(parent) = node.parent() {
                    let mut name = get_node_text(&node, content_bytes).to_string();
                    let namespace = get_namespace(&parent, content_bytes);
                    
                    if *capture_name == "enum_name" && name == "Type" {
                        if let Some(ns) = &namespace {
                            name = format!("{}::{}", ns, name);
                        }
                    }

                    let mut symbol_type = "class";
                    if *capture_name == "struct_name" { symbol_type = "struct"; }
                    if *capture_name == "enum_name" { symbol_type = "enum"; }
                    
                    let kind_str = parent.kind();
                    if kind_str == "unreal_class_declaration" { symbol_type = "UCLASS"; }
                    else if kind_str == "unreal_struct_declaration" { symbol_type = "USTRUCT"; }
                    else if kind_str == "unreal_enum_declaration" { symbol_type = "UENUM"; }

                    let range_start = parent.start_byte();
                    let range_end = parent.end_byte();
                    
                    let mut exists = false;
                    for c in &classes {
                        if c.range_start == range_start && c.range_end == range_end {
                            exists = true;
                            break;
                        }
                    }

                    if !name.is_empty() && !exists {
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
        } else if *capture_name == "macro_item_name" {
            if let Some(parent) = node.parent() { 
                if let Some(macro_node) = parent.parent() { 
                    let mut current = macro_node;
                    while current.kind() != "unreal_declaration_macro" && current.parent().is_some() {
                        current = current.parent().unwrap();
                    }
                    
                    if current.kind() == "unreal_declaration_macro" {
                        let name = get_node_text(&node, content_bytes).to_string();
                        let namespace = get_namespace(&current, content_bytes);
                        
                        let mut macro_type_str = "delegate";
                        if let Some(type_node) = current.child_by_field_name("name") {
                            macro_type_str = get_node_text(&type_node, content_bytes);
                        }

                        let is_delegate = macro_type_str.starts_with("DECLARE_DELEGATE") ||
                                          macro_type_str.starts_with("DECLARE_MULTICAST_DELEGATE") ||
                                          macro_type_str.starts_with("DECLARE_DYNAMIC_DELEGATE") ||
                                          macro_type_str.starts_with("DECLARE_DYNAMIC_MULTICAST") ||
                                          macro_type_str.starts_with("DECLARE_EVENT") ||
                                          macro_type_str.starts_with("DECLARE_TS_MULTICAST_DELEGATE");

                        if is_delegate && !name.is_empty() {
                            let mut is_name_position = false;
                            let has_retval = macro_type_str.contains("RetVal");
                            
                            if let Some(spec_node) = parent.parent() { 
                                if let Some(list_node) = spec_node.parent() { 
                                    let mut arg_idx = 0;
                                    let mut cursor = list_node.walk();
                                    for child in list_node.children(&mut cursor) {
                                        if child.id() == spec_node.id() {
                                            is_name_position = if has_retval { arg_idx == 2 } else { arg_idx == 0 };
                                            break;
                                        }
                                        if child.kind() == "unreal_specifier" {
                                            arg_idx += 2; 
                                        }
                                    }
                                }
                            }

                            if is_name_position {
                                classes.push(ClassInfo {
                                    class_name: name,
                                    namespace,
                                    base_classes: vec![macro_type_str.to_string()],
                                    symbol_type: "struct".to_string(),
                                    line: node.start_position().row + 1,
                                    range_start: current.start_byte(),
                                    range_end: current.end_byte(),
                                    members: Vec::new(),
                                    is_final: false,
                                    is_interface: false,
                                });
                            }
                        }
                    }
                }
            }
        } else if *capture_name == "alias_decl" {
            if let Some(name_node) = node.child_by_field_name("name") {
                 let name = get_node_text(&name_node, content_bytes).to_string();
                 let namespace = get_namespace(&node, content_bytes);
                 
                 if let Some(type_node) = node.child_by_field_name("type") {
                     let mut target_type = get_node_text(&type_node, content_bytes).to_string();
                     if let Some(idx) = target_type.find('<') { target_type = target_type[..idx].to_string(); }
                     target_type = target_type.trim().to_string();
                     
                     if !name.is_empty() && !target_type.is_empty() && name != target_type {
                         classes.push(ClassInfo {
                            class_name: name, namespace, base_classes: vec![target_type], symbol_type: "typedef".to_string(),
                            line: node.start_position().row + 1, range_start: node.start_byte(), range_end: node.end_byte(),
                            members: Vec::new(), is_final: false, is_interface: false,
                         });
                     }
                 }
            }
        } else if *capture_name == "typedef_decl" {
            if let Some(name_node) = node.child_by_field_name("declarator") {
                 let name = get_node_text(&name_node, content_bytes).to_string();
                 if !name.contains('(') && !name.contains('<') && !name.contains(':') && !name.contains(' ') {
                     let namespace = get_namespace(&node, content_bytes);
                     if let Some(type_node) = node.child_by_field_name("type") {
                         let mut target_type = get_node_text(&type_node, content_bytes).to_string();
                         if let Some(idx) = target_type.find('<') { target_type = target_type[..idx].to_string(); }
                         target_type = target_type.trim().to_string();
                         
                         if !name.is_empty() && !target_type.is_empty() && name != target_type {
                             classes.push(ClassInfo {
                                class_name: name, namespace, base_classes: vec![target_type], symbol_type: "typedef".to_string(),
                                line: node.start_position().row + 1, range_start: node.start_byte(), range_end: node.end_byte(),
                                members: Vec::new(), is_final: false, is_interface: false,
                             });
                         }
                     }
                 }
            }
        } else if *capture_name == "base_class_name" {
            let node_start = node.start_byte();
            if let Some(cls) = classes.last_mut() {
                if node_start >= cls.range_start && node_start <= cls.range_end {
                    let mut name = get_node_text(&node, content_bytes).to_string();
                    if let Some(idx) = name.rfind("::") {
                        name = name[idx+2..].to_string();
                    }
                    if name != cls.class_name {
                        cls.base_classes.push(name);
                    }
                }
            }
        } else if *capture_name == "func_node" || *capture_name == "decl_node" || *capture_name == "ufunc_node" || *capture_name == "field_node" {
            let definition_node = node;
            
            // Extract member name and scope
            let mut member_name = String::new();
            let mut scope_name = None;
            let mut is_function = *capture_name == "func_node" || *capture_name == "ufunc_node";
            
            // Traverse to find name and scope
            if let Some(declarator) = find_declarator_node(definition_node) {
                let mut current = declarator;
                loop {
                    match current.kind() {
                        "identifier" | "field_identifier" => {
                            member_name = get_node_text(&current, content_bytes).to_string();
                            break;
                        },
                        "qualified_identifier" => {
                            if let Some(s) = current.child_by_field_name("scope") {
                                scope_name = Some(get_node_text(&s, content_bytes).to_string());
                            }
                            if let Some(n) = current.child_by_field_name("name") {
                                member_name = get_node_text(&n, content_bytes).to_string();
                            }
                            break;
                        },
                        "function_declarator" => {
                            is_function = true;
                            if let Some(d) = current.child_by_field_name("declarator") {
                                current = d;
                                continue;
                            }
                            break;
                        },
                        "pointer_declarator" | "reference_declarator" | "array_declarator" => {
                            if let Some(d) = current.child_by_field_name("declarator") {
                                current = d;
                                continue;
                            }
                            break;
                        },
                        _ => break,
                    }
                }
            }

            if member_name.is_empty() { continue; }

            let mut flags = Vec::new();
            let mut access = "public".to_string();
            
            if has_child_type(definition_node, "ufunction_macro") || has_child_type(definition_node, "unreal_function_macro") || definition_node.kind() == "unreal_function_declaration" {
                flags.push("UFUNCTION");
                is_function = true;
            }
            if has_child_type(definition_node, "uproperty_macro") || has_child_type(definition_node, "unreal_property_macro") {
                flags.push("UPROPERTY");
                is_function = false;
            }
            
            // Access specifier check
            let mut curr = definition_node;
            while let Some(parent) = curr.parent() {
                let pk = parent.kind();
                if pk == "field_declaration_list" || pk == "class_specifier" || pk == "struct_specifier" {
                    let mut cursor = parent.walk();
                    for child in parent.children(&mut cursor) {
                        if child.start_byte() >= curr.start_byte() { break; }
                        if child.kind() == "access_specifier" {
                            access = get_node_text(&child, content_bytes).trim().trim_end_matches(':').trim().to_lowercase();
                        }
                    }
                    break;
                }
                curr = parent;
            }

            let node_text = get_node_text(&definition_node, content_bytes);
            if node_text.contains("virtual") { flags.push("virtual"); }
            if node_text.contains("static") { flags.push("static"); }
            if node_text.contains("override") { flags.push("override"); }

            let mut detail = None;
            let mut return_type = None;
            let mem_type = if is_function { "function" } else { "property" };

            // Return type extraction
            if let Some(idx) = node_text.find(&member_name) {
                let prefix = &node_text[..idx];
                let mut actual_prefix = prefix;
                if let Some(macro_end) = prefix.rfind(')') {
                    actual_prefix = &prefix[macro_end+1..];
                }
                let mut cleaned = clean_type_string(actual_prefix);
                
                // Strip class scope from out-of-line definitions (e.g., "bool MyClass::" -> "bool")
                if let Some(sn) = &scope_name {
                    let scope_marker = format!("{}::", sn);
                    if let Some(s_idx) = cleaned.find(&scope_marker) {
                        cleaned = cleaned[..s_idx].trim().to_string();
                    }
                }

                if !cleaned.is_empty() { return_type = Some(cleaned); }
            }

            if is_function {
                if let Some(param_list) = find_child_by_type(definition_node, "parameter_list") {
                    detail = Some(get_node_text(&param_list, content_bytes).to_string());
                }
            }

            if !member_name.is_empty() && member_name != "virtual" && member_name != "static" && member_name != "void" && member_name != "const" {
                let mut member = MemberInfo {
                    name: member_name.clone(),
                    mem_type: mem_type.to_string(),
                    flags: flags.join(" "),
                    access,
                    line: definition_node.start_position().row + 1,
                    detail,
                    return_type,
                };

                if let Some(sn) = scope_name {
                    let mut found_idx = None;
                    for (i, cls) in classes.iter().enumerate() { if cls.class_name == sn { found_idx = Some(i); break; } }
                    
                    let idx = if let Some(i) = found_idx { i } else {
                        classes.push(ClassInfo {
                            class_name: sn.clone(), namespace: None, base_classes: Vec::new(), symbol_type: "class".to_string(),
                            line: 1, range_start: 0, range_end: 0, members: Vec::new(), is_final: false, is_interface: false,
                        });
                        classes.len() - 1
                    };
                    
                    member.access = "impl".to_string();
                    classes[idx].members.push(member);
                } else {
                    members.push((member, definition_node.start_byte(), definition_node.end_byte()));
                }
            }
        } else if *capture_name == "enum_val_name" {
            let name = get_node_text(&node, content_bytes).to_string();
            if !name.is_empty() {
                members.push((MemberInfo {
                    name,
                    mem_type: "enum_item".to_string(),
                    flags: String::new(),
                    access: "public".to_string(),
                    line: node.start_position().row + 1,
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
        module_id: input.module_id,
    })
}

// --- Internal Helpers ---

fn get_node_text<'a>(node: &Node, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

fn get_namespace<'a>(node: &Node<'a>, source: &'a [u8]) -> Option<String> {
    let mut ns_parts = Vec::new();
    let mut current = node.parent();
    
    while let Some(n) = current {
        let kind = n.kind();
        if kind == "namespace_definition" || kind == "class_specifier" || kind == "struct_specifier" {
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

fn find_declarator_node<'a>(node: Node<'a>) -> Option<Node<'a>> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            let field_name = node.field_name_for_child(i as u32);
            if field_name == Some("declarator") {
                return Some(child);
            }
            if let Some(found) = find_declarator_node(child) {
                return Some(found);
            }
        }
    }
    None
}

fn clean_type_string(s: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    for word in s.split_whitespace() {
        let w = word.trim();
        if w.is_empty() { continue; }
        
        if w == "virtual" || w == "static" || w == "inline" || w == "FORCEINLINE" || 
           w == "FORCEINLINE_DEBUGGABLE" ||
           w == "const" || w == "friend" || w == "class" || w == "struct" || w == "enum" ||
           w.starts_with("UE_DEPRECATED") || 
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
