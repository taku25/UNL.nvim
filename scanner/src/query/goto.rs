use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{json, Value};
use std::collections::HashMap;
use tree_sitter::{Parser, Point};
use crate::db::path::PATH_CTE;

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn node_text<'a>(node: &tree_sitter::Node, src: &'a [u8]) -> &'a str {
    std::str::from_utf8(&src[node.byte_range()]).unwrap_or("")
}

fn children_of<'a>(node: &tree_sitter::Node<'a>) -> Vec<tree_sitter::Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

fn find_descendant_of_kind<'a>(
    node: tree_sitter::Node<'a>,
    kind: &str,
) -> Option<tree_sitter::Node<'a>> {
    if node.kind() == kind {
        return Some(node);
    }
    for child in children_of(&node) {
        if let Some(found) = find_descendant_of_kind(child, kind) {
            return Some(found);
        }
    }
    None
}

/// カーソルが属する囲みクラス / 構造体名を返す
fn get_enclosing_class(node: tree_sitter::Node, src: &[u8]) -> Option<String> {
    let mut cur = Some(node);
    while let Some(n) = cur {
        match n.kind() {
            "class_specifier" | "struct_specifier"
            | "unreal_class_declaration" | "unreal_struct_declaration" => {
                if let Some(name_node) = n.child_by_field_name("name") {
                    let t = node_text(&name_node, src).trim().to_string();
                    if !t.is_empty() {
                        return Some(t);
                    }
                }
            }
            "function_definition" => {
                // void AMyActor::Tick(...) {} → scope is "AMyActor"
                if let Some(decl) = n.child_by_field_name("declarator") {
                    if let Some(qi) = find_descendant_of_kind(decl, "qualified_identifier") {
                        if let Some(scope) = qi.child_by_field_name("scope") {
                            let t = node_text(&scope, src).trim().to_string();
                            if !t.is_empty() {
                                return Some(t);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        cur = n.parent();
    }
    None
}

// ---------------------------------------------------------------------------
// Cursor context extraction
// ---------------------------------------------------------------------------

pub struct CursorCtx {
    pub symbol: String,
    /// the text before :: or . or ->
    pub qualifier: Option<String>,
    /// "::", ".", or "->"
    pub qualifier_op: Option<String>,
    pub enclosing_class: Option<String>,
}

pub fn extract_cursor_context(content: &str, line: u32, character: u32) -> Option<CursorCtx> {
    let language: tree_sitter::Language = tree_sitter_unreal_cpp::LANGUAGE.into();
    let mut parser = Parser::new();
    parser.set_language(&language).ok()?;
    let tree = parser.parse(content, None)?;
    let root = tree.root_node();
    let src = content.as_bytes();

    let row = line as usize;
    let col = character as usize;
    let node = root.descendant_for_point_range(
        Point::new(row, if col > 0 { col - 1 } else { 0 }),
        Point::new(row, col),
    )?;

    let symbol = node_text(&node, src).trim().to_string();
    if symbol.is_empty() || node.is_extra() {
        return None;
    }

    let enclosing_class = get_enclosing_class(node, src);

    let mut qualifier: Option<String> = None;
    let mut qualifier_op: Option<String> = None;

    let mut cur = node.parent();
    while let Some(n) = cur {
        match n.kind() {
            "qualified_identifier" => {
                if let Some(scope) = n.child_by_field_name("scope") {
                    let t = node_text(&scope, src).trim().to_string();
                    if !t.is_empty() {
                        qualifier = Some(t);
                        qualifier_op = Some("::".to_string());
                    }
                }
                break;
            }
            "field_expression" => {
                let children = children_of(&n);
                for (i, child) in children.iter().enumerate() {
                    let ck = child.kind();
                    if ck == "." || ck == "->" {
                        if i > 0 {
                            let obj_text = node_text(&children[i - 1], src).trim().to_string();
                            if !obj_text.is_empty() {
                                qualifier = Some(obj_text);
                                qualifier_op = Some(ck.to_string());
                            }
                        }
                        break;
                    }
                }
                break;
            }
            _ => {}
        }
        cur = n.parent();
    }

    Some(CursorCtx { symbol, qualifier, qualifier_op, enclosing_class })
}

// ---------------------------------------------------------------------------
// Type inference from buffer
// ---------------------------------------------------------------------------

/// バッファ内の宣言から変数の型名を推論する
pub fn infer_var_type(content: &str, var_name: &str) -> Option<String> {
    let language: tree_sitter::Language = tree_sitter_unreal_cpp::LANGUAGE.into();
    let mut parser = Parser::new();
    parser.set_language(&language).ok()?;
    let tree = parser.parse(content, None)?;
    let root = tree.root_node();
    let src = content.as_bytes();
    scan_for_decl(root, src, var_name)
}

fn scan_for_decl(node: tree_sitter::Node, src: &[u8], var_name: &str) -> Option<String> {
    match node.kind() {
        "declaration" | "parameter_declaration" => {
            if let Some(type_node) = node.child_by_field_name("type") {
                if let Some(decl_node) = node.child_by_field_name("declarator") {
                    if let Some(name) = extract_decl_name(decl_node, src) {
                        if name == var_name {
                            let raw = node_text(&type_node, src).trim().to_string();
                            return Some(clean_type(&raw));
                        }
                    }
                }
            }
        }
        _ => {}
    }
    for child in children_of(&node) {
        if let Some(result) = scan_for_decl(child, src, var_name) {
            return Some(result);
        }
    }
    None
}

fn extract_decl_name(node: tree_sitter::Node, src: &[u8]) -> Option<String> {
    match node.kind() {
        "identifier" | "field_identifier" => {
            Some(node_text(&node, src).trim().to_string())
        }
        "pointer_declarator" | "reference_declarator" => {
            for child in children_of(&node) {
                if let Some(name) = extract_decl_name(child, src) {
                    return Some(name);
                }
            }
            None
        }
        _ => {
            if let Some(d) = node.child_by_field_name("declarator") {
                return extract_decl_name(d, src);
            }
            for child in children_of(&node) {
                if matches!(child.kind(), "identifier" | "field_identifier") {
                    return Some(node_text(&child, src).trim().to_string());
                }
            }
            None
        }
    }
}

/// `TObjectPtr<X>`, `const X*`, `X&` などから基底型名 X を抽出する
fn clean_type(t: &str) -> String {
    let t = t.trim();
    if let Some(start) = t.find('<') {
        if let Some(end) = t.rfind('>') {
            return clean_type(&t[start + 1..end]);
        }
    }
    t.trim_start_matches("const ")
        .trim()
        .trim_end_matches('*')
        .trim_end_matches('&')
        .trim()
        .to_string()
}

// ---------------------------------------------------------------------------
// DB helper
// ---------------------------------------------------------------------------

struct GotoCtx<'a> {
    conn: &'a Connection,
    class_id_cache: HashMap<String, Vec<i64>>,
}

impl<'a> GotoCtx<'a> {
    fn new(conn: &'a Connection) -> Self {
        Self {
            conn,
            class_id_cache: HashMap::new(),
        }
    }

    /// クラス名に対応する classes.id を返す。
    /// ヘッダーファイル (.h/.hpp) に紐づくエントリを先頭に並べる。
    fn get_class_ids(&mut self, name: &str) -> anyhow::Result<Vec<i64>> {
        let name = name.trim();
        if name.is_empty() {
            return Ok(Vec::new());
        }
        if let Some(ids) = self.class_id_cache.get(name) {
            return Ok(ids.clone());
        }
        let mut stmt = self.conn.prepare(
            "SELECT c.id FROM classes c
             JOIN strings s  ON c.name_id = s.id
             JOIN files f    ON c.file_id = f.id
             JOIN strings sf ON f.filename_id = sf.id
             WHERE s.text = ?
             ORDER BY CASE
               WHEN sf.text LIKE '%.h'   THEN 0
               WHEN sf.text LIKE '%.hpp' THEN 1
               ELSE 2
             END",
        )?;
        let ids: Vec<i64> = stmt
            .query_map([name], |r| r.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        self.class_id_cache.insert(name.to_string(), ids.clone());
        Ok(ids)
    }
}

// ---------------------------------------------------------------------------
// Public query functions
// ---------------------------------------------------------------------------

/// 継承チェーンを BFS で辿ってメンバーの定義箇所を返す
pub fn find_symbol_in_inheritance_chain(
    conn: &Connection,
    class_name: &str,
    symbol_name: &str,
) -> anyhow::Result<Option<Value>> {
    let mut ctx = GotoCtx::new(conn);
    let start_ids = ctx.get_class_ids(class_name)?;
    if start_ids.is_empty() {
        return Ok(None);
    }

    let member_sql = format!(
        "{} SELECT sm.text, m.line_number, dp.full_path || '/' || sf.text, sc.text
         FROM members m
         JOIN strings sm ON m.name_id = sm.id
         JOIN classes c ON m.class_id = c.id
         JOIN strings sc ON c.name_id = sc.id
         JOIN files f ON COALESCE(m.file_id, c.file_id) = f.id
         JOIN dir_paths dp ON f.directory_id = dp.id
         JOIN strings sf ON f.filename_id = sf.id
         WHERE m.class_id = ? AND sm.text = ?
         ORDER BY
           CASE WHEN m.access = 'impl' THEN 1 ELSE 0 END,
           CASE
             WHEN sf.text LIKE '%.h'   THEN 0
             WHEN sf.text LIKE '%.hpp' THEN 1
             ELSE 2
           END
         LIMIT 1",
        PATH_CTE
    );

    let mut queue: std::collections::VecDeque<i64> = start_ids.into_iter().collect();
    let mut visited: HashMap<i64, bool> = HashMap::new();

    while let Some(cls_id) = queue.pop_front() {
        if visited.contains_key(&cls_id) {
            continue;
        }
        visited.insert(cls_id, true);

        let res = conn
            .query_row(&member_sql, params![cls_id, symbol_name], |row| {
                Ok(json!({
                    "symbol_name": row.get::<_, String>(0)?,
                    "line_number": row.get::<_, i64>(1)?,
                    "file_path":   row.get::<_, String>(2)?,
                    "class_name":  row.get::<_, String>(3)?,
                }))
            })
            .optional()?;

        if let Some(r) = res {
            return Ok(Some(r));
        }

        // 親クラスを BFS キューへ追加 (FIFO)
        let parents: Vec<(Option<i64>, String)> = conn
            .prepare(
                "SELECT parent_class_id, si.text FROM inheritance i \
                 JOIN strings si ON i.parent_name_id = si.id WHERE child_id = ?",
            )?
            .query_map([cls_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        for (p_id, p_name) in parents {
            if let Some(id) = p_id {
                if !visited.contains_key(&id) {
                    queue.push_back(id);
                }
            }
            for id in ctx.get_class_ids(&p_name)? {
                if !visited.contains_key(&id) {
                    queue.push_back(id);
                }
            }
        }
    }

    Ok(None)
}

/// モジュール内でシンボルを検索する（クラス定義 → メンバーの順）
pub fn find_symbol_in_module(
    conn: &Connection,
    module: &str,
    symbol: &str,
) -> anyhow::Result<Option<Value>> {
    let class_sql = format!(
        "{} SELECT sc.text, c.line_number, dp.full_path || '/' || sf.text
         FROM classes c
         JOIN strings sc ON c.name_id = sc.id
         JOIN files f ON c.file_id = f.id
         JOIN dir_paths dp ON f.directory_id = dp.id
         JOIN strings sf ON f.filename_id = sf.id
         JOIN modules m ON f.module_id = m.id
         JOIN strings sm ON m.name_id = sm.id
         WHERE sm.text = ? AND sc.text = ?
         LIMIT 1",
        PATH_CTE
    );
    let class_result = conn
        .query_row(&class_sql, params![module, symbol], |row| {
            Ok(json!({
                "symbol_name": row.get::<_, String>(0)?,
                "line_number": row.get::<_, i64>(1)?,
                "file_path":   row.get::<_, String>(2)?,
            }))
        })
        .optional()?;
    if class_result.is_some() {
        return Ok(class_result);
    }

    let member_sql = format!(
        "{} SELECT sm.text, mem.line_number, dp.full_path || '/' || sf.text
         FROM members mem
         JOIN strings sm ON mem.name_id = sm.id
         JOIN classes c ON mem.class_id = c.id
         JOIN files f ON COALESCE(mem.file_id, c.file_id) = f.id
         JOIN dir_paths dp ON f.directory_id = dp.id
         JOIN strings sf ON f.filename_id = sf.id
         JOIN modules m ON c.file_id = f.id
         JOIN strings mods ON m.name_id = mods.id
         WHERE mods.text = ? AND sm.text = ?
         ORDER BY CASE
           WHEN sf.text LIKE '%.h'   THEN 0
           WHEN sf.text LIKE '%.hpp' THEN 1
           ELSE 2
         END
         LIMIT 1",
        PATH_CTE
    );
    let member_result = conn
        .query_row(&member_sql, params![module, symbol], |row| {
            Ok(json!({
                "symbol_name": row.get::<_, String>(0)?,
                "line_number": row.get::<_, i64>(1)?,
                "file_path":   row.get::<_, String>(2)?,
            }))
        })
        .optional()?;
    Ok(member_result)
}

/// クラス / 構造体 / Enum の定義場所を返す
fn find_type_definition(conn: &Connection, name: &str) -> anyhow::Result<Option<Value>> {
    let sql = format!(
        "{} SELECT sc.text, c.line_number, dp.full_path || '/' || sf.text
         FROM classes c
         JOIN strings sc ON c.name_id = sc.id
         JOIN files f ON c.file_id = f.id
         JOIN dir_paths dp ON f.directory_id = dp.id
         JOIN strings sf ON f.filename_id = sf.id
         WHERE sc.text = ?
         ORDER BY CASE
           WHEN sf.text LIKE '%.h'   THEN 0
           WHEN sf.text LIKE '%.hpp' THEN 1
           ELSE 2
         END
         LIMIT 1",
        PATH_CTE
    );
    let result = conn
        .query_row(&sql, [name], |row| {
            Ok(json!({
                "symbol_name": row.get::<_, String>(0)?,
                "line_number": row.get::<_, i64>(1)?,
                "file_path":   row.get::<_, String>(2)?,
                "class_name":  row.get::<_, String>(0)?,
            }))
        })
        .optional()?;
    Ok(result)
}

/// 全クラスからメンバー名で検索（最終フォールバック）
fn find_member_anywhere(conn: &Connection, symbol_name: &str) -> anyhow::Result<Option<Value>> {
    let sql = format!(
        "{} SELECT sm.text, m.line_number, dp.full_path || '/' || sf.text, sc.text
         FROM members m
         JOIN strings sm ON m.name_id = sm.id
         JOIN classes c ON m.class_id = c.id
         JOIN strings sc ON c.name_id = sc.id
         JOIN files f ON COALESCE(m.file_id, c.file_id) = f.id
         JOIN dir_paths dp ON f.directory_id = dp.id
         JOIN strings sf ON f.filename_id = sf.id
         WHERE sm.text = ?
         ORDER BY
           CASE WHEN m.access = 'impl' THEN 1 ELSE 0 END,
           CASE
             WHEN sf.text LIKE '%.h'   THEN 0
             WHEN sf.text LIKE '%.hpp' THEN 1
             ELSE 2
           END
         LIMIT 1",
        PATH_CTE
    );
    let result = conn
        .query_row(&sql, [symbol_name], |row| {
            Ok(json!({
                "symbol_name": row.get::<_, String>(0)?,
                "line_number": row.get::<_, i64>(1)?,
                "file_path":   row.get::<_, String>(2)?,
                "class_name":  row.get::<_, String>(3)?,
            }))
        })
        .optional()?;
    Ok(result)
}

// ---------------------------------------------------------------------------
// Local definition search
// ---------------------------------------------------------------------------

fn search_decl_in_node(node: tree_sitter::Node, src: &[u8], symbol: &str) -> Option<Point> {
    match node.kind() {
        "declaration" | "field_declaration" => {
            if let Some(decl_node) = node.child_by_field_name("declarator") {
                if let Some(name) = extract_decl_name(decl_node, src) {
                    if name == symbol {
                        return Some(decl_node.start_position());
                    }
                }
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "init_declarator" {
                    if let Some(pos) = search_decl_in_node(child, src, symbol) {
                        return Some(pos);
                    }
                }
            }
        }
        "parameter_declaration" | "init_declarator" | "function_definition" => {
            if let Some(decl_node) = node.child_by_field_name("declarator") {
                if let Some(name) = extract_decl_name(decl_node, src) {
                    if name == symbol {
                        return Some(decl_node.start_position());
                    }
                }
            }
        }
        "parameter_list" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if let Some(pos) = search_decl_in_node(child, src, symbol) {
                    return Some(pos);
                }
            }
        }
        "enumerator" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                if node_text(&name_node, src) == symbol {
                    return Some(name_node.start_position());
                }
            }
        }
        "class_specifier" | "struct_specifier" | "enum_specifier"
        | "unreal_class_declaration" | "unreal_struct_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                if node_text(&name_node, src) == symbol {
                    return Some(name_node.start_position());
                }
            }
        }
        _ => {}
    }
    None
}

fn find_local_definition(
    root: tree_sitter::Node,
    src: &[u8],
    symbol: &str,
    cursor_point: Point,
) -> Option<Point> {
    let mut curr = root.descendant_for_point_range(cursor_point, cursor_point);
    while let Some(n) = curr {
        if let Some(parent) = n.parent() {
            let mut cursor = parent.walk();
            for child in parent.children(&mut cursor) {
                if child.start_position() > cursor_point {
                    let pk = parent.kind();
                    if pk != "class_specifier"
                        && pk != "struct_specifier"
                        && pk != "field_declaration_list"
                    {
                        break;
                    }
                }
                if let Some(pos) = search_decl_in_node(child, src, symbol) {
                    if pos != cursor_point {
                        return Some(pos);
                    }
                }
            }
        }
        if n.kind() == "function_definition" {
            if let Some(decl) = n.child_by_field_name("declarator") {
                if let Some(params) = find_descendant_of_kind(decl, "parameter_list") {
                    if let Some(pos) = search_decl_in_node(params, src, symbol) {
                        if pos != cursor_point {
                            return Some(pos);
                        }
                    }
                }
            }
        }
        curr = n.parent();
    }
    None
}

// ---------------------------------------------------------------------------
// Main entry
// ---------------------------------------------------------------------------

/// GotoDefinition のメインロジック
///
/// 戻り値: `{ file_path, line_number, symbol_name, class_name }` または `null`
pub fn goto_definition(
    conn: &Connection,
    content: String,
    line: u32,
    character: u32,
    file_path: Option<String>,
) -> anyhow::Result<Value> {
    let language: tree_sitter::Language = tree_sitter_unreal_cpp::LANGUAGE.into();
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .map_err(|e| anyhow::anyhow!("Failed to load language: {}", e))?;
    let tree = parser
        .parse(&content, None)
        .ok_or_else(|| anyhow::anyhow!("Failed to parse content"))?;
    let root = tree.root_node();
    let src = content.as_bytes();

    let ctx = match extract_cursor_context(&content, line, character) {
        Some(c) => c,
        None => return Ok(Value::Null),
    };

    tracing::debug!(
        "GotoDefinition: symbol='{}' qualifier={:?} op={:?} enclosing={:?}",
        ctx.symbol,
        ctx.qualifier,
        ctx.qualifier_op,
        ctx.enclosing_class,
    );

    // 0. ローカル（カレントバッファ内）の定義を優先的に検索
    let cursor_point = Point::new(line as usize, character as usize);
    if let Some(pos) = find_local_definition(root, src, &ctx.symbol, cursor_point) {
        tracing::debug!("Found local definition for '{}' at {:?}", ctx.symbol, pos);
        return Ok(json!({
            "symbol_name": ctx.symbol,
            "line_number": pos.row + 1,
            "file_path":   file_path.unwrap_or_default(),
            "class_name":  ctx.enclosing_class,
        }));
    }

    // 1. 明示的な修飾子がある場合
    if let Some(ref qual) = ctx.qualifier {
        let class_name = match ctx.qualifier_op.as_deref() {
            Some("::") => qual.clone(),
            Some("." | "->") => {
                // obj.Method() / ptr->Method() → obj の型を推論
                infer_var_type(&content, qual).unwrap_or_else(|| qual.clone())
            }
            _ => qual.clone(),
        };

        tracing::debug!("Qualifier resolved to class: '{}'", class_name);

        if let Some(result) = find_symbol_in_inheritance_chain(conn, &class_name, &ctx.symbol)? {
            return Ok(result);
        }

        // 型定義として試す（例: AMyActor::StaticClass() の "StaticClass" が失敗した場合など）
        if let Some(result) = find_type_definition(conn, &ctx.symbol)? {
            return Ok(result);
        }
    }

    // 2. 囲むクラスのメンバーとして検索
    if let Some(ref enc) = ctx.enclosing_class {
        if let Some(result) = find_symbol_in_inheritance_chain(conn, enc, &ctx.symbol)? {
            return Ok(result);
        }
    }

    // 3. クラス / 構造体 / enum の定義として検索
    if let Some(result) = find_type_definition(conn, &ctx.symbol)? {
        return Ok(result);
    }

    // 4. 全メンバーから名前で検索（最終フォールバック）
    if let Some(result) = find_member_anywhere(conn, &ctx.symbol)? {
        return Ok(result);
    }

    tracing::debug!("GotoDefinition: '{}' not found", ctx.symbol);
    Ok(Value::Null)
}
