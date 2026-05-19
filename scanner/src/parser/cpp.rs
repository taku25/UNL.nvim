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
  (alias_declaration) @alias_node
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
            content_bytes.windows(7).any(|w| w == b"UCLASS(" || w == b"USTRUCT" || w == b"UENUM(" || w == b"DECLARE" || w == b"include" || w == b"#define")
            || content_bytes.windows(10).any(|w| w == b"UFUNCTION" || w == b"UPROPERTY")
            || content_bytes.windows(12).any(|w| w == b"GAMEPLAY_TAG");

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
            let mut members: Vec<(MemberInfo, usize, usize, bool)> = Vec::new();

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
                                let raw_prefix = std::str::from_utf8(actual_prefix).unwrap_or("");
                                // static キーワードを flags に反映する（前置修飾子から検出）
                                if raw_prefix.split_whitespace().any(|w| w == "static") {
                                    flags.push("static");
                                }
                                let cleaned = clean_type_string(raw_prefix);
                                if !cleaned.is_empty() { return_type = Some(cleaned); }
                            }
                        }
                        // declarator_node がない場合（field_declaration 等）は
                        // storage_class_specifier ノードを直接確認する
                        if !flags.contains(&"static") {
                            if has_static_specifier(node, content_bytes) {
                                flags.push("static");
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
                            } else {
                                let in_compound = is_inside_compound_statement(node);
                                members.push((member, node.start_byte(), node.end_byte(), in_compound));
                            }
                        }
                    }
                } else if *capture_name == "alias_node" {
                    // using X = SomeType; をメンバーとして登録
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let alias_name = get_node_text(&name_node, content_bytes).to_string();
                        if !alias_name.is_empty() {
                            let alias_type = node.child_by_field_name("type")
                                .map(|t| get_node_text(&t, content_bytes).to_string())
                                .unwrap_or_default();
                            let mut access = "public".to_string();
                            let mut curr = node;
                            while let Some(parent) = curr.parent() {
                                let pk = parent.kind();
                                if pk == "field_declaration_list" || pk == "class_specifier" || pk == "struct_specifier" {
                                    let mut walk = parent.walk();
                                    for child in parent.children(&mut walk) {
                                        if child.start_byte() >= curr.start_byte() { break; }
                                        if child.kind() == "access_specifier" {
                                            access = get_node_text(&child, content_bytes).trim().trim_end_matches(':').to_lowercase();
                                        }
                                    }
                                    break;
                                }
                                curr = parent;
                            }
                            let member = MemberInfo {
                                name: alias_name,
                                mem_type: "type_alias".to_string(),
                                flags: "".to_string(),
                                access,
                                line: node.start_position().row + 1,
                                end_line: node.end_position().row + 1,
                                detail: if alias_type.is_empty() { None } else { Some(alias_type.clone()) },
                                return_type: if alias_type.is_empty() { None } else { Some(alias_type) },
                            };
                            members.push((member, node.start_byte(), node.end_byte(), is_inside_compound_statement(node)));
                        }
                    }
                } else if *capture_name == "enum_val_name" {
                    members.push((MemberInfo { name: get_node_text(&node, content_bytes).to_string(), mem_type: "enum_item".to_string(), flags: "".to_string(), access: "public".to_string(), line: node.start_position().row + 1, end_line: node.end_position().row + 1, detail: None, return_type: None }, node.start_byte(), node.end_byte(), false));
                }
            }
            for (member, m_start, m_end, in_compound) in members {
                if let Some(idx) = classes.iter().enumerate().filter(|(_, c)| m_start >= c.range_start && m_end <= c.range_end).min_by_key(|(_, c)| c.range_end - c.range_start).map(|(i, _)| i) {
                    classes[idx].members.push(member);
                } else if !in_compound {
                    // クラス・構造体の外でかつ関数ボディの外 → グローバルシンボルとして登録
                    let symbol_type = match member.mem_type.as_str() {
                        "function"   => "global_function",
                        "type_alias" => "type_alias",
                        _            => "global_var",
                    };
                    if !member.name.is_empty() {
                        classes.push(ClassInfo {
                            class_name: member.name.clone(),
                            namespace: None,
                            base_classes: vec![],
                            symbol_type: symbol_type.to_string(),
                            line: member.line,
                            end_line: member.end_line,
                            range_start: m_start,
                            range_end: m_end,
                            members: vec![],
                            is_final: false,
                            is_interface: false,
                        });
                    }
                }
            }

            // UE_DEFINE_GAMEPLAY_TAG_COMMENT / UE_DEFINE_GAMEPLAY_TAG /
            // UE_DECLARE_GAMEPLAY_TAG_EXTERN が含まれる namespace を ClassInfo として登録する。
            // これにより BS2GameplayTags::E000100:: 形式の補完が機能するようになる。
            scan_gameplay_tag_namespaces(root, content_bytes, &mut classes);
            scan_preproc_defines(root, content_bytes, &mut classes);
            scan_delegate_and_log_macros(root, content_bytes, &mut classes);

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

/// ノードの直接の子に `storage_class_specifier` として "static" があるか確認する。
fn has_static_specifier(node: Node, source: &[u8]) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "storage_class_specifier" && get_node_text(&child, source).trim() == "static" {
            return true;
        }
    }
    false
}

fn find_declarator_node<'a>(node: Node<'a>) -> Option<Node<'a>> {
    for i in 0..node.child_count() {
        let fname = node.field_name_for_child(i as u32);
        if fname == Some("declarator") { return node.child(i as u32); }
        // Only recurse into unnamed children.
        // Named fields like "type", "body", "arguments" must NOT be recursed because
        // they may contain their own "declarator" fields (e.g. type_descriptor inside
        // TArray<class Foo*> has field:declarator: abstract_pointer_declarator)
        // which would be mistakenly returned instead of the actual member declarator.
        if fname.is_none() {
            if let Some(child) = node.child(i as u32) {
                if let Some(found) = find_declarator_node(child) { return Some(found); }
            }
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

// ─── Global scope helpers ────────────────────────────────────────────────────

/// ノードが `compound_statement`（関数ボディ等）の内側にあるか判定する。
/// これが true のとき、そのノードはローカルスコープにある。
fn is_inside_compound_statement(node: Node) -> bool {
    let mut curr = node;
    while let Some(parent) = curr.parent() {
        match parent.kind() {
            "compound_statement" => return true,
            "translation_unit"   => return false,
            _ => {}
        }
        curr = parent;
    }
    false
}

// ─── #define macro scanning ─────────────────────────────────────────────────
/// `translation_unit` の直接の子 `preproc_def` / `preproc_function_def` ノードから
/// マクロ名を抽出し、`ClassInfo { symbol_type: "define" }` としてグローバルシンボルに登録する。
/// 中身の値は格納しない（名前のみ）。
fn scan_preproc_defines(root: Node, content_bytes: &[u8], classes: &mut Vec<ClassInfo>) {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        let kind = child.kind();
        if kind != "preproc_def" && kind != "preproc_function_def" {
            continue;
        }
        if let Some(name_node) = child.child_by_field_name("name") {
            let macro_name = get_node_text(&name_node, content_bytes).trim().to_string();
            if macro_name.is_empty() {
                continue;
            }
            classes.push(ClassInfo {
                class_name: macro_name,
                namespace: None,
                base_classes: vec![],
                symbol_type: "define".to_string(),
                line: child.start_position().row + 1,
                end_line: child.end_position().row + 1,
                range_start: child.start_byte(),
                range_end: child.end_byte(),
                members: vec![],
                is_final: false,
                is_interface: false,
            });
        }
    }
}

// ─── UE Gameplay Tag namespace scanning ────────────────────────────────────────

/// `namespace_definition` ノードを再帰的に探し、`UE_DEFINE_GAMEPLAY_TAG_COMMENT` /
/// `UE_DEFINE_GAMEPLAY_TAG` / `UE_DECLARE_GAMEPLAY_TAG_EXTERN` マクロ呼び出しを含む
/// namespaceを `ClassInfo { symbol_type: "namespace" }` として登録する。
/// これにより `BS2GameplayTags::E000100::` の補完が機能する。
fn scan_gameplay_tag_namespaces(root: Node, content_bytes: &[u8], classes: &mut Vec<ClassInfo>) {
    // 再帰ではなく明示的なスタックで反復処理する。
    // C++ では namespace_definition は translation_unit か別の namespace の直接の子にしか現れないため、
    // namespace_definition ノードだけを追いかければ十分。
    let mut stack: Vec<Node> = vec![root];
    while let Some(node) = stack.pop() {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "namespace_definition" {
                continue;
            }
            // このnamespaceのメンバーを収集
            if let Some(body) = child.child_by_field_name("body") {
                let members = collect_direct_tag_members(body, content_bytes);
                if !members.is_empty() {
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let name_text = get_node_text(&name_node, content_bytes).trim().to_string();
                        let parts: Vec<&str> = name_text.split("::").collect();
                        let local_class_name = parts.last().copied().unwrap_or(&name_text).to_string();
                        let local_ns_prefix: Option<String> = if parts.len() > 1 {
                            Some(parts[..parts.len() - 1].join("::"))
                        } else {
                            None
                        };
                        let ancestor_ns = get_ancestor_namespace(&child, content_bytes);
                        let namespace = match (ancestor_ns, local_ns_prefix) {
                            (Some(a), Some(l)) => Some(format!("{}::{}", a, l)),
                            (Some(a), None)    => Some(a),
                            (None,    Some(l)) => Some(l),
                            (None,    None)    => None,
                        };
                        classes.push(ClassInfo {
                            class_name: local_class_name,
                            namespace,
                            base_classes: vec![],
                            symbol_type: "namespace".to_string(),
                            line: child.start_position().row + 1,
                            end_line: child.end_position().row + 1,
                            range_start: child.start_byte(),
                            range_end: child.end_byte(),
                            members,
                            is_final: false,
                            is_interface: false,
                        });
                    }
                }
                // ネストした namespace を処理するためにボディをスタックに積む
                stack.push(body);
            }
        }
    }
}

/// namespace body の直接の子ノードのみから GameplayTag 関連シンボルを収集する。
/// 子 `namespace_definition` ノードには入らない（`scan_namespace_node` が別途処理する）。
///
/// 以下の２パターンを処理する：
/// 1. マクロ呼び出し:
///      UE_DEFINE_GAMEPLAY_TAG_COMMENT(VarName, "tag.string", "comment")
///      UE_DEFINE_GAMEPLAY_TAG(VarName, "tag.string")
///      UE_DECLARE_GAMEPLAY_TAG_EXTERN(VarName)
/// 2. extern 変数宣言 (ヘッダーの FNativeGameplayTag 形式):
///      extern FNativeGameplayTag VarName;
///      extern FGameplayTag VarName;
fn collect_direct_tag_members(body: Node, content_bytes: &[u8]) -> Vec<MemberInfo> {
    let mut members = Vec::new();
    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        // 子 namespace には入らない
        if child.kind() == "namespace_definition" {
            continue;
        }
        // パターン 1: UE_DEFINE_GAMEPLAY_TAG_COMMENT / UE_DEFINE_GAMEPLAY_TAG / UE_DECLARE_GAMEPLAY_TAG_EXTERN
        if let Some(call_node) = find_direct_call(child) {
            if let Some((var_name, detail)) = extract_tag_from_call(call_node, content_bytes) {
                members.push(MemberInfo {
                    name: var_name,
                    mem_type: "property".to_string(),
                    // "static" フラグで is_static=1 として DB に登録される。
                    // namespace スコープの変数は :: でアクセスするため static メンバと同扱いにする。
                    flags: "static".to_string(),
                    access: "public".to_string(),
                    line: call_node.start_position().row + 1,
                    end_line: call_node.end_position().row + 1,
                    detail: if detail.is_empty() { None } else { Some(detail) },
                    return_type: Some("FGameplayTag".to_string()),
                });
                continue;
            }
        }
        // パターン 2: extern FNativeGameplayTag / FGameplayTag VarName;
        if let Some((var_name, type_name)) = extract_extern_tag_decl(child, content_bytes) {
            members.push(MemberInfo {
                name: var_name,
                mem_type: "property".to_string(),
                flags: "static".to_string(),
                access: "public".to_string(),
                line: child.start_position().row + 1,
                end_line: child.end_position().row + 1,
                detail: None,
                return_type: Some(type_name),
            });
        }
    }
    members
}

/// ノード自身が call_expression か、直接の子に call_expression を持つ場合に返す。
fn find_direct_call(node: Node) -> Option<Node> {
    if node.kind() == "call_expression" {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "call_expression" {
            return Some(child);
        }
    }
    None
}

/// call_expression が GameplayTag マクロ呼び出しであれば
/// `(変数名, タグ文字列)` を返す。
/// - UE_DEFINE_GAMEPLAY_TAG_COMMENT(VarName, "tag.string", "comment") → 3 args
/// - UE_DEFINE_GAMEPLAY_TAG(VarName, "tag.string")                    → 2 args
/// - UE_DECLARE_GAMEPLAY_TAG_EXTERN(VarName)                          → 1 arg
fn extract_tag_from_call<'a>(call: Node<'a>, content_bytes: &'a [u8]) -> Option<(String, String)> {
    let func_node = call.child_by_field_name("function")?;
    let func_name = get_node_text(&func_node, content_bytes).trim();
    if !func_name.starts_with("UE_DEFINE_GAMEPLAY_TAG") && !func_name.starts_with("UE_DECLARE_GAMEPLAY_TAG") {
        return None;
    }

    let args_node = call.child_by_field_name("arguments")?;
    let mut cursor = args_node.walk();
    let mut arg_count = 0usize;
    let mut var_name = String::new();
    let mut tag_string = String::new();
    let mut comment = String::new();

    for child in args_node.children(&mut cursor) {
        let k = child.kind();
        if k == "(" || k == ")" || k == "," {
            continue;
        }
        match arg_count {
            0 => var_name  = get_node_text(&child, content_bytes).trim().to_string(),
            1 => tag_string = get_node_text(&child, content_bytes).trim().trim_matches('"').to_string(),
            2 => comment   = get_node_text(&child, content_bytes).trim().trim_matches('"').to_string(),
            _ => {}
        }
        arg_count += 1;
        if arg_count >= 3 { break; }
    }

    if var_name.is_empty() { return None; }

    // detail = "tag.string" または "tag.string — comment"
    let detail = if !comment.is_empty() {
        format!("{} — {}", tag_string, comment)
    } else {
        tag_string
    };
    Some((var_name, detail))
}

/// 祖先の `namespace_definition` ノードの名前を `::` 結合で返す。
/// `get_namespace` と同様だが `namespace_definition` のみを対象とする。
fn get_ancestor_namespace(node: &Node, source: &[u8]) -> Option<String> {
    let mut parts = Vec::new();
    let mut curr = node.parent();
    while let Some(n) = curr {
        if n.kind() == "namespace_definition" {
            if let Some(name) = n.child_by_field_name("name") {
                parts.push(get_node_text(&name, source).trim().to_string());
            }
        }
        curr = n.parent();
    }
    if parts.is_empty() { None } else { parts.reverse(); Some(parts.join("::")) }
}

/// `extern FNativeGameplayTag VarName;` / `extern FGameplayTag VarName;` 形式の宣言から
/// (変数名, 型名) を抽出する。GameplayTag 型以外の extern 宣言はスキップする。
fn extract_extern_tag_decl(node: Node, content_bytes: &[u8]) -> Option<(String, String)> {
    if node.kind() != "declaration" {
        return None;
    }
    // storage class specifier "extern" を持つかチェック
    let has_extern = {
        let mut cursor = node.walk();
        let result = node.children(&mut cursor).any(|c| {
            c.kind() == "storage_class_specifier" && get_node_text(&c, content_bytes) == "extern"
        });
        result
    };
    if !has_extern {
        return None;
    }
    // type フィールドが GameplayTag 関連型かチェック
    let type_node = node.child_by_field_name("type")?;
    let type_text = get_node_text(&type_node, content_bytes).trim().to_string();
    // FNativeGameplayTag, FGameplayTag, FNativeGameplayTagComment などを受け入れる
    if !type_text.contains("GameplayTag") {
        return None;
    }
    // declarator フィールドから変数名を取得
    let declarator = node.child_by_field_name("declarator")?;
    let var_name = get_node_text(&declarator, content_bytes).trim().to_string();
    if var_name.is_empty() {
        return None;
    }
    Some((var_name, type_text))
}

// ─── Delegate / Log category macro scanning ─────────────────────────────────

/// デリゲート宣言マクロ (`DECLARE_DELEGATE_*`, `DECLARE_EVENT_*` 等) と
/// ログカテゴリ宣言マクロ (`DECLARE_LOG_CATEGORY_EXTERN`, `DEFINE_LOG_CATEGORY*`) を
/// `translation_unit` の直接の子から収集する。
fn scan_delegate_and_log_macros(root: Node, content_bytes: &[u8], classes: &mut Vec<ClassInfo>) {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        match child.kind() {
            "unreal_declaration_macro" => {
                if let Some((name, sym_type)) = extract_unreal_decl_macro(child, content_bytes) {
                    classes.push(ClassInfo {
                        class_name:   name,
                        namespace:    None,
                        base_classes: vec![],
                        symbol_type:  sym_type,
                        line:         child.start_position().row + 1,
                        end_line:     child.end_position().row + 1,
                        range_start:  child.start_byte(),
                        range_end:    child.end_byte(),
                        members:      vec![],
                        is_final:     false,
                        is_interface: false,
                    });
                }
            }
            "expression_statement" | "declaration" => {
                if let Some((name, sym_type)) = extract_log_category_node(child, content_bytes) {
                    classes.push(ClassInfo {
                        class_name:   name,
                        namespace:    None,
                        base_classes: vec![],
                        symbol_type:  sym_type,
                        line:         child.start_position().row + 1,
                        end_line:     child.end_position().row + 1,
                        range_start:  child.start_byte(),
                        range_end:    child.end_byte(),
                        members:      vec![],
                        is_final:     false,
                        is_interface: false,
                    });
                }
            }
            _ => {}
        }
    }
}

fn extract_unreal_decl_macro(node: Node, content_bytes: &[u8]) -> Option<(String, String)> {
    let macro_name_node = node.child_by_field_name("name")?;
    let macro_name = get_node_text(&macro_name_node, content_bytes).trim().to_string();

    let is_delegate = macro_name.starts_with("DECLARE_DELEGATE")
        || macro_name.starts_with("DECLARE_DYNAMIC_DELEGATE")
        || macro_name.starts_with("DECLARE_MULTICAST_DELEGATE")
        || macro_name.starts_with("DECLARE_DYNAMIC_MULTICAST_DELEGATE")
        || macro_name.starts_with("DECLARE_SPARSE_DYNAMIC_DELEGATE")
        || macro_name.starts_with("DECLARE_TS_MULTICAST_DELEGATE");
    let is_event = macro_name.starts_with("DECLARE_EVENT");

    if !is_delegate && !is_event {
        return None;
    }

    // RetVal 系 / Event 系はデリゲート名が引数 1 番目
    let name_arg_idx: usize = if macro_name.contains("RetVal") || is_event { 1 } else { 0 };

    let args_node = node.child_by_field_name("arguments")?;
    let delegate_name = get_nth_specifier_identifier(args_node, name_arg_idx, content_bytes)?;

    Some((delegate_name, "delegate".to_string()))
}

fn get_nth_specifier_identifier(args: Node, n: usize, content_bytes: &[u8]) -> Option<String> {
    let mut cursor = args.walk();
    let mut idx = 0usize;
    for child in args.children(&mut cursor) {
        let k = child.kind();
        if k == "(" || k == ")" || k == "," {
            continue;
        }
        if idx == n {
            let text = get_leaf_identifier_text(child, content_bytes);
            if !text.is_empty() { return Some(text); }
            let raw = get_node_text(&child, content_bytes).trim().to_string();
            if !raw.is_empty() { return Some(raw); }
            return None;
        }
        idx += 1;
    }
    None
}

fn get_leaf_identifier_text(node: Node, content_bytes: &[u8]) -> String {
    if node.child_count() == 0 {
        return get_node_text(&node, content_bytes).trim().to_string();
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let text = get_leaf_identifier_text(child, content_bytes);
        if !text.is_empty() { return text; }
    }
    String::new()
}

fn extract_log_category_node(node: Node, content_bytes: &[u8]) -> Option<(String, String)> {
    let call = find_direct_call(node)?;
    let func_node = call.child_by_field_name("function")?;
    let func_name = get_node_text(&func_node, content_bytes).trim().to_string();
    if func_name != "DECLARE_LOG_CATEGORY_EXTERN"
        && func_name != "DEFINE_LOG_CATEGORY"
        && func_name != "DEFINE_LOG_CATEGORY_STATIC"
    {
        return None;
    }
    let args_node = call.child_by_field_name("arguments")?;
    let log_name = get_nth_specifier_identifier(args_node, 0, content_bytes)?;
    Some((log_name, "log_category".to_string()))
}
