use std::fs::File;
use std::cell::RefCell;
use std::path::Path;
use std::sync::OnceLock;
use tree_sitter::{Parser, Query, QueryCursor, Node};
use streaming_iterator::StreamingIterator;
use sha2::{Sha256, Digest};
use memmap2::Mmap;
use regex::Regex;
use crate::types::{InputFile, ParseResult, ParseData, ClassInfo, MemberInfo};

struct CleanRegexes {
    keywords: Vec<Regex>,
    api: Regex,
    macros: Regex,
}

static CLEAN_REGEXES: OnceLock<CleanRegexes> = OnceLock::new();

fn get_clean_regexes() -> &'static CleanRegexes {
    CLEAN_REGEXES.get_or_init(|| {
        let kws = ["virtual","static","inline","FORCEINLINE","FORCEINLINE_DEBUGGABLE",
                   "const","friend","class","struct","enum","typename"];
        CleanRegexes {
            keywords: kws.iter().map(|kw| Regex::new(&format!(r"\b{}\b", kw)).unwrap()).collect(),
            api:    Regex::new(r"\b[A-Z0-9_]+_API\b").unwrap(),
            macros: Regex::new(r"UFUNCTION\(.*?\)|UPROPERTY\(.*?\)").unwrap(),
        }
    })
}

thread_local! {
    static PARSER: RefCell<Parser> = RefCell::new(Parser::new());
    static CURSOR: RefCell<QueryCursor> = RefCell::new(QueryCursor::new());
}

pub const QUERY_STR: &str = r#"
  (class_specifier name: (type_identifier) @class_name) @class_def
  (struct_specifier name: (type_identifier) @struct_name) @struct_def
  (enum_specifier name: (type_identifier) @enum_name) @enum_def
  (unreal_class_declaration name: (type_identifier) @class_name) @uclass_def
  (unreal_struct_declaration name: (_) @struct_name) @ustruct_def
  (unreal_enum_declaration name: (_) @enum_name) @uenum_def
  (base_class_clause (access_specifier)? (type_identifier) @base_class_name)
  (function_definition) @func_node
  (declaration) @decl_node
  (unreal_function_declaration) @ufunc_node
  (field_declaration) @field_node
  (enumerator name: (identifier) @enum_val_name) @enum_item
  (call_expression
    function: [
      (identifier) @call_name
      (field_expression field: (field_identifier) @call_name)
    ]
  ) @call_expr
  (field_expression field: (field_identifier) @call_name) @field_expr
"#;

pub const INCLUDE_QUERY_STR: &str = "(preproc_include path: [(string_literal) @path (system_lib_string) @path]) @include";

pub fn process_file(input: &InputFile, language: &tree_sitter::Language, query: &Query, include_query: &Query) -> anyhow::Result<ParseResult> {
    let file = File::open(&input.path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    let content_bytes = &mmap[..];
    
    let mut hasher = Sha256::new();
    hasher.update(content_bytes);
    let new_hash = format!("{:x}", hasher.finalize());

    if let Some(old) = &input.old_hash {
        if old == &new_hash {
            return Ok(ParseResult {
                path: input.path.clone(), status: "cache_hit".to_string(),
                mtime: input.mtime, data: None, module_id: input.module_id,
            });
        }
    }

    // 拡張子のチェック
    let ext = Path::new(&input.path).extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    let is_header = matches!(ext.as_str(), "h" | "hpp" | "inl");

    if is_header {
        // ヘッダーファイルのみ、高速フィルタリングを適用
        let has_important_keywords = 
            content_bytes.windows(7).any(|w| w == b"UCLASS(" || w == b"USTRUCT" || w == b"UENUM(" || w == b"DECLARE" || w == b"include") 
            || content_bytes.windows(10).any(|w| w == b"UFUNCTION" || w == b"UPROPERTY");

        if !has_important_keywords {
            return Ok(ParseResult {
                path: input.path.clone(), status: "parsed".to_string(), mtime: input.mtime,
                data: Some(ParseData { classes: vec![], calls: vec![], includes: vec![], parser: "fast-skip".to_string(), new_hash }),
                module_id: input.module_id,
            });
        }
    }

    // .cpp や重要なヘッダーはパース実行
    let (classes, calls, includes) = parse_content_mmap(content_bytes, &input.path, language, query, include_query)?;

    Ok(ParseResult {
        path: input.path.clone(), status: "parsed".to_string(), mtime: input.mtime,
        data: Some(ParseData { classes, calls, includes, parser: "treesitter".to_string(), new_hash }),
        module_id: input.module_id,
    })
}

pub fn parse_content_mmap(content_bytes: &[u8], _path: &str, language: &tree_sitter::Language, query: &Query, include_query: &Query) -> anyhow::Result<(Vec<ClassInfo>, Vec<crate::types::CallInfo>, Vec<String>)> {
    PARSER.with(|p_cell| {
        let mut parser = p_cell.borrow_mut();
        parser.set_language(language).unwrap();
        
        let tree = parser.parse(content_bytes, None).ok_or(anyhow::anyhow!("Parse failed"))?;
        let root = tree.root_node();
        
        CURSOR.with(|c_cell| {
            let mut cursor = c_cell.borrow_mut();
            
            let mut classes: Vec<ClassInfo> = Vec::new();
            let mut calls: Vec<crate::types::CallInfo> = Vec::new();
            let mut includes: Vec<String> = Vec::new();
            let mut members: Vec<(MemberInfo, usize, usize)> = Vec::new();

            // インクルード解析 (渡された include_query を使用)
            let mut include_matches = cursor.matches(include_query, root, content_bytes);
            while let Some(m) = include_matches.next() {
                for cap in m.captures {
                    if include_query.capture_names()[cap.index as usize] == "path" {
                        let path_text = get_node_text(&cap.node, content_bytes).trim_matches('"').trim_matches('<').trim_matches('>').to_string();
                        if !path_text.is_empty() { includes.push(path_text); }
                    }
                }
            }

            // シンボル解析
            let mut captures = cursor.captures(query, root, content_bytes);
            while let Some((m, capture_index)) = captures.next() {
                let capture = m.captures[*capture_index];
                let capture_name = &query.capture_names()[capture.index as usize];
                let node = capture.node;
                
                if *capture_name == "call_name" {
                    let name = get_node_text(&node, content_bytes).to_string();
                    if !name.is_empty() { calls.push(crate::types::CallInfo { name, line: node.start_position().row + 1 }); }
                    continue;
                }

                if *capture_name == "class_name" || *capture_name == "struct_name" || *capture_name == "enum_name" {
                    if let Some(parent) = node.parent() {
                        if parent.child_by_field_name("body").is_some() {
                            let mut name = get_node_text(&node, content_bytes).to_string();
                            let namespace = get_namespace(&parent, content_bytes);
                            if *capture_name == "enum_name" && name == "Type" { if let Some(ns) = &namespace { name = format!("{}::{}", ns, name); } }

                            let mut symbol_type = match *capture_name { "struct_name" => "struct", "enum_name" => "enum", _ => "class" };
                            let kind_str = parent.kind();
                            if kind_str == "unreal_class_declaration" { symbol_type = "UCLASS"; }
                            else if kind_str == "unreal_struct_declaration" { symbol_type = "USTRUCT"; }
                            else if kind_str == "unreal_enum_declaration" { symbol_type = "UENUM"; }

                            classes.push(ClassInfo {
                                class_name: name, namespace, base_classes: Vec::new(), symbol_type: symbol_type.to_string(),
                                line: node.start_position().row + 1, end_line: parent.end_position().row + 1,
                                range_start: parent.start_byte(), range_end: parent.end_byte(),
                                members: Vec::new(), is_final: false, is_interface: false,
                            });
                        }
                    }
                } else if *capture_name == "base_class_name" {
                    let node_start = node.start_byte();
                    if let Some(cls) = classes.last_mut() {
                        if node_start >= cls.range_start && node_start <= cls.range_end {
                            let mut name = get_node_text(&node, content_bytes).to_string();
                            if let Some(idx) = name.rfind("::") { name = name[idx+2..].to_string(); }
                            if name != cls.class_name { cls.base_classes.push(name); }
                        }
                    }
                } else if matches!(*capture_name, "func_node" | "decl_node" | "ufunc_node" | "field_node") {
                    let mut member_name = String::new();
                    let mut scope_name = None;
                    let mut is_function = matches!(*capture_name, "func_node" | "ufunc_node");
                    let mut declarator_node: Option<Node> = None;
                    
                    if let Some(declarator) = find_declarator_node(node) {
                        declarator_node = Some(declarator);
                        let mut current = declarator;
                        loop {
                            match current.kind() {
                                "identifier" | "field_identifier" => { member_name = get_node_text(&current, content_bytes).to_string(); break; },
                                "qualified_identifier" => {
                                    if let Some(s) = current.child_by_field_name("scope") { scope_name = Some(get_node_text(&s, content_bytes).to_string()); }
                                    if let Some(n) = current.child_by_field_name("name") { member_name = get_node_text(&n, content_bytes).to_string(); }
                                    break;
                                },
                                "function_declarator" => { is_function = true; if let Some(d) = current.child_by_field_name("declarator") { current = d; continue; } break; },
                                "pointer_declarator" | "reference_declarator" | "array_declarator" => { if let Some(d) = current.child_by_field_name("declarator") { current = d; continue; } break; },
                                _ => break,
                            }
                        }
                    }

                    if !member_name.is_empty() {
                        let mut flags = Vec::new();
                        let mut access = if scope_name.is_some() && is_function { "impl".to_string() } else { "public".to_string() };
                        if has_child_type(node, "ufunction_macro") || node.kind() == "unreal_function_declaration" { flags.push("UFUNCTION"); is_function = true; }
                        if has_child_type(node, "uproperty_macro") { flags.push("UPROPERTY"); is_function = false; }
                        
                        let mut curr = node;
                        while let Some(parent) = curr.parent() {
                            let pk = parent.kind();
                            if pk == "field_declaration_list" || pk == "class_specifier" || pk == "struct_specifier" {
                                let mut cursor = parent.walk();
                                for child in parent.children(&mut cursor) {
                                    if child.start_byte() >= curr.start_byte() { break; }
                                    if child.kind() == "access_specifier" { access = get_node_text(&child, content_bytes).trim().trim_end_matches(':').to_lowercase(); }
                                }
                                break;
                            }
                            curr = parent;
                        }

                        let mut return_type = None;
                        if let Some(decl) = declarator_node {
                            let (start, end) = (node.start_byte(), decl.start_byte());
                            if end > start {
                                let mut actual_prefix = &content_bytes[start..end];
                                if let Some(idx) = actual_prefix.iter().rposition(|&b| b == b')') { actual_prefix = &actual_prefix[idx+1..]; }
                                let cleaned = clean_type_string(std::str::from_utf8(actual_prefix).unwrap_or(""));
                                if !cleaned.is_empty() { return_type = Some(cleaned); }
                            }
                        }

                        let mut detail = None;
                        if is_function { if let Some(params) = find_child_by_type(node, "parameter_list") { detail = Some(get_node_text(&params, content_bytes).to_string()); } }

                        if !["virtual", "static", "void", "const"].contains(&member_name.as_str()) {
                            let mut member = MemberInfo { name: member_name.clone(), mem_type: (if is_function { "function" } else { "property" }).to_string(), flags: flags.join(" "), access, line: node.start_position().row + 1, end_line: node.end_position().row + 1, detail, return_type };
                            if let Some(sn) = scope_name {
                                let idx = classes.iter().position(|c| c.class_name == sn).unwrap_or_else(|| {
                                    classes.push(ClassInfo { class_name: sn.clone(), namespace: None, base_classes: vec![], symbol_type: "class".to_string(), line: 1, end_line: 999999, range_start: 0, range_end: 0, members: vec![], is_final: false, is_interface: false });
                                    classes.len() - 1
                                });
                                member.access = "impl".to_string();
                                classes[idx].members.push(member);
                            } else { members.push((member, node.start_byte(), node.end_byte())); }
                        }
                    }
                } else if *capture_name == "enum_val_name" {
                    members.push((MemberInfo { name: get_node_text(&node, content_bytes).to_string(), mem_type: "enum_item".to_string(), flags: "".to_string(), access: "public".to_string(), line: node.start_position().row + 1, end_line: node.end_position().row + 1, detail: None, return_type: None }, node.start_byte(), node.end_byte()));
                }
            }
            for (member, m_start, m_end) in members {
                if let Some(idx) = classes.iter().enumerate().filter(|(_, c)| m_start >= c.range_start && m_end <= c.range_end).min_by_key(|(_, c)| c.range_end - c.range_start).map(|(i, _)| i) {
                    classes[idx].members.push(member);
                }
            }
            Ok((classes, calls, includes))
        })
    })
}

pub fn parse_content(content: &str, path: &str, language: &tree_sitter::Language, query: &Query) -> anyhow::Result<(Vec<ClassInfo>, Vec<crate::types::CallInfo>, Vec<String>)> {
    let include_query = Query::new(language, INCLUDE_QUERY_STR).unwrap();
    parse_content_mmap(content.as_bytes(), path, language, query, &include_query)
}

fn get_node_text<'a>(node: &Node, source: &'a [u8]) -> &'a str { node.utf8_text(source).unwrap_or("") }

fn get_namespace<'a>(node: &Node<'a>, source: &'a [u8]) -> Option<String> {
    let mut parts = Vec::new();
    let mut curr = node.parent();
    while let Some(n) = curr {
        if matches!(n.kind(), "namespace_definition" | "class_specifier" | "struct_specifier") {
            if let Some(name) = n.child_by_field_name("name") { parts.push(get_node_text(&name, source).to_string()); }
        }
        curr = n.parent();
    }
    if parts.is_empty() { None } else { parts.reverse(); Some(parts.join("::")) }
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
        if node.field_name_for_child(i as u32) == Some("declarator") { return node.child(i as u32); }
        if let Some(child) = node.child(i as u32) {
            if let Some(found) = find_declarator_node(child) { return Some(found); }
        }
    }
    None
}

fn clean_type_string(s: &str) -> String {
    let cr = get_clean_regexes();
    let mut clean = s.trim().to_string();
    for re in &cr.keywords { clean = re.replace_all(&clean, "").to_string(); }
    clean = cr.api.replace_all(&clean, "").to_string();
    clean = cr.macros.replace_all(&clean, "").to_string();
    clean = clean.replace(";", "").replace(":", " : ").replace("  ", " ").trim().to_string();
    if clean.contains('<') && clean.contains('>') { return clean; }
    clean.split_whitespace().last().unwrap_or("").to_string()
}
