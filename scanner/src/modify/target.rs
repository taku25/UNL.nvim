//! Replicates the logic of `UEP.nvim/lua/UEP/parser/target.lua`.
//!
//! Parses a `.Target.cs` C# file using tree-sitter-c-sharp, then:
//!  1. Ensures `RegisterModulesCreatedByNeovim()` is called in the constructor.
//!  2. Ensures the `RegisterModulesCreatedByNeovim` private method exists and
//!     contains the named module in its `ExtraModuleNames.AddRange` call.

use anyhow::{anyhow, Result};
use tree_sitter::{Node, Parser, Query, QueryCursor};
use streaming_iterator::StreamingIterator;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn add_module(file_path: &str, module_name: &str) -> Result<()> {
    let raw = std::fs::read_to_string(file_path)?;
    let mut lines: Vec<String> = raw.lines().map(|l| l.to_string()).collect();

    let language: tree_sitter::Language = tree_sitter_c_sharp::LANGUAGE.into();
    let mut parser = Parser::new();
    parser.set_language(&language)?;

    let source = raw.as_bytes();
    let tree = parser.parse(source, None).ok_or_else(|| anyhow!("tree-sitter failed to parse"))?;
    let root = tree.root_node();

    let tabstop = get_tabstop(root, source, &lines)?;
    let mut offset: usize = 0;

    // Step 1: if RegisterModulesCreatedByNeovim() is not yet called in the
    // constructor body, insert the call after the last statement there.
    if !constructor_has_call(root, source)? {
        let last_expr = constructor_last_expression(root, source)?
            .ok_or_else(|| anyhow!("Constructor has no statements"))?;
        let end_row = last_expr.end_position().row;
        let t2 = format!("{}{}", tabstop, tabstop);
        lines.insert(end_row + 1, format!("{}RegisterModulesCreatedByNeovim();", t2));
        offset += 1;
    }

    // Step 2: ensure the method exists and contains the module.
    let method_infos = method_declared(root, source, module_name)?;

    let t1 = &tabstop;
    let t2 = format!("{}{}", t1, t1);
    let t3 = format!("{}{}{}", t1, t1, t1);

    if method_infos.method_found {
        if method_infos.add_range_found {
            if !method_infos.module_found {
                match method_infos.last_module {
                    Some(last_mod) => {
                        let end_row = last_mod.end_position().row + offset;
                        let end_col = last_mod.end_position().column;
                        let line_len = lines[end_row].len();
                        if line_len != end_col {
                            let trailing = lines[end_row][end_col..].to_string();
                            lines.insert(end_row + 1, format!("{}{}", t3, trailing));
                            offset += 1;
                        }
                        let prefix = lines[end_row][..end_col].to_string();
                        lines[end_row] = format!("{},", prefix);
                        lines.insert(end_row + 1, format!("{}\"{}\"", t3, module_name));
                        offset += 1;
                    }
                    None => {
                        let init = method_infos.initializer_expr
                            .ok_or_else(|| anyhow!("initializer_expr missing"))?;
                        let open_child = init.child(0)
                            .ok_or_else(|| anyhow!("initializer_expr has no children"))?;
                        let close_child = init.child((init.child_count() - 1) as u32)
                            .ok_or_else(|| anyhow!("initializer_expr has one child only"))?;
                        let endr_i = open_child.end_position().row + offset;
                        let endc_i = open_child.end_position().column;
                        let startc_f = close_child.start_position().column;
                        let endr_f = close_child.end_position().row + offset;
                        if endr_i == endr_f {
                            let trailing = lines[endr_i][startc_f..].to_string();
                            lines.insert(endr_i + 1, format!("{}{}", t2, trailing));
                            lines[endr_i] = lines[endr_i][..endc_i].to_string();
                            lines.insert(endr_i + 1, format!("{}\"{}\"", t3, module_name));
                        } else {
                            lines.insert(endr_f, format!("{}\"{}\"", t3, module_name));
                        }
                    }
                }
            }
        } else {
            let last_expr_row = method_infos.method_last_expr
                .map(|n| n.end_position().row + offset)
                .ok_or_else(|| anyhow!("method_last_expr missing"))?;
            lines.insert(last_expr_row + 1, format!("{}}});", t2));
            lines.insert(last_expr_row + 1, format!("{}\"{}\"", t3, module_name));
            lines.insert(last_expr_row + 1, format!("{}ExtraModuleNames.AddRange(new string[] {{", t2));
            offset += 3;
        }
    } else {
        let last_decl_row = method_infos.last_decl
            .map(|n| n.end_position().row + offset)
            .ok_or_else(|| anyhow!("last_decl missing"))?;
        lines.insert(last_decl_row + 1, format!("{}}}", t1));
        lines.insert(last_decl_row + 1, format!("{}}});", t2));
        lines.insert(last_decl_row + 1, format!("{}\"{}\"", t3, module_name));
        lines.insert(last_decl_row + 1, format!("{}ExtraModuleNames.AddRange(new string[] {{", t2));
        lines.insert(last_decl_row + 1, format!("{}{{", t1));
        lines.insert(last_decl_row + 1, format!("{}private void RegisterModulesCreatedByNeovim()", t1));
        offset += 6;
    }

    let backup = format!("{}.old", file_path);
    std::fs::copy(file_path, &backup)?;
    std::fs::write(file_path, lines.join("\n"))?;
    let _ = offset;
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn node_text<'a>(node: Node, source: &'a [u8]) -> &'a str {
    let start = node.start_byte();
    let end = node.end_byte();
    std::str::from_utf8(&source[start..end]).unwrap_or("")
}

fn get_tabstop(root: Node, source: &[u8], lines: &[String]) -> Result<String> {
    let q_src = r#"
( compilation_unit
  ( class_declaration
    body: ( declaration_list
      ( constructor_declaration ) @constr.decl
    )
  )
)
"#;
    let language: tree_sitter::Language = tree_sitter_c_sharp::LANGUAGE.into();
    let q = Query::new(&language, q_src)?;
    let cap_names = q.capture_names();
    let idx_constr = cap_names.iter().position(|n| *n == "constr.decl")
        .ok_or_else(|| anyhow!("constr.decl not found"))? as u32;
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&q, root, source);
    while let Some(m) = matches.next() {
        for cap in m.captures {
            if cap.index == idx_constr {
                let row = cap.node.start_position().row;
                let col = cap.node.start_position().column;
                return Ok(lines[row][..col].to_string());
            }
        }
    }
    Ok("    ".to_string())
}

fn constructor_has_call(root: Node, source: &[u8]) -> Result<bool> {
    let q_src = r#"
( compilation_unit
  ( class_declaration
    ( base_list ( identifier ) @class.base )
    body: ( declaration_list
      ( constructor_declaration
        body: ( block
          ( expression_statement
            ( invocation_expression
              function: ( identifier ) @nvim.call
            )
          )
        )
      )
    )
  )
)
"#;
    let language: tree_sitter::Language = tree_sitter_c_sharp::LANGUAGE.into();
    let q = Query::new(&language, q_src)?;
    let cap_names = q.capture_names();
    let idx_base = cap_names.iter().position(|n| *n == "class.base").map(|i| i as u32);
    let idx_call = cap_names.iter().position(|n| *n == "nvim.call").map(|i| i as u32);
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&q, root, source);
    while let Some(m) = matches.next() {
        let has_target = m.captures.iter().any(|c| {
            Some(c.index) == idx_base && node_text(c.node, source) == "TargetRules"
        });
        let has_call = m.captures.iter().any(|c| {
            Some(c.index) == idx_call
                && node_text(c.node, source) == "RegisterModulesCreatedByNeovim"
        });
        if has_target && has_call {
            return Ok(true);
        }
    }
    Ok(false)
}

fn constructor_last_expression<'a>(root: Node<'a>, source: &[u8]) -> Result<Option<Node<'a>>> {
    let q_src = r#"
( compilation_unit
  ( class_declaration
    ( base_list ( identifier ) @class.base )
    body: ( declaration_list
      ( constructor_declaration
        body: ( block (_) @constr.last . )
      )
    )
  )
)
"#;
    let language: tree_sitter::Language = tree_sitter_c_sharp::LANGUAGE.into();
    let q = Query::new(&language, q_src)?;
    let cap_names = q.capture_names();
    let idx_base = cap_names.iter().position(|n| *n == "class.base").map(|i| i as u32);
    let idx_last = cap_names.iter().position(|n| *n == "constr.last").map(|i| i as u32);
    let mut result: Option<Node> = None;
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&q, root, source);
    while let Some(m) = matches.next() {
        let has_target = m.captures.iter().any(|c| {
            Some(c.index) == idx_base && node_text(c.node, source) == "TargetRules"
        });
        if !has_target { continue; }
        for cap in m.captures {
            if Some(cap.index) == idx_last {
                result = Some(cap.node);
            }
        }
    }
    Ok(result)
}

struct MethodInfos {
    method_found: bool,
    add_range_found: bool,
    module_found: bool,
    method_last_expr: Option<tree_sitter::Node<'static>>,
    initializer_expr: Option<tree_sitter::Node<'static>>,
    last_module: Option<tree_sitter::Node<'static>>,
    last_decl: Option<tree_sitter::Node<'static>>,
}

fn method_declared(root: Node, source: &[u8], module_name: &str) -> Result<MethodInfos> {
    // Safety: tree lives for the duration of this function; the struct
    // is consumed before we return.
    let root_s: Node<'static> = unsafe { std::mem::transmute(root) };

    let language: tree_sitter::Language = tree_sitter_c_sharp::LANGUAGE.into();
    let mut result = MethodInfos {
        method_found: false,
        add_range_found: false,
        module_found: false,
        method_last_expr: None,
        initializer_expr: None,
        last_module: None,
        last_decl: None,
    };

    // Query 1: method existence + last statement
    {
        let q = Query::new(&language, r#"
( compilation_unit
  ( class_declaration
    ( base_list ( identifier ) @class.base )
    body: ( declaration_list
      ( method_declaration
        name: ( identifier ) @method.def
        body: ( block (_)* @last.expr . )
      )
    )
  )
)
"#)?;
        let cap_names = q.capture_names();
        let idx_base  = cap_names.iter().position(|n| *n == "class.base").map(|i| i as u32);
        let idx_mdef  = cap_names.iter().position(|n| *n == "method.def").map(|i| i as u32);
        let idx_lexpr = cap_names.iter().position(|n| *n == "last.expr").map(|i| i as u32);
        let mut cursor = QueryCursor::new();
        let mut ms = cursor.matches(&q, root_s, source);
        while let Some(m) = ms.next() {
            let is_target = m.captures.iter().any(|c| {
                Some(c.index) == idx_base && node_text(c.node, source) == "TargetRules"
            });
            if !is_target { continue; }
            let is_nvim = m.captures.iter().any(|c| {
                Some(c.index) == idx_mdef
                    && node_text(c.node, source) == "RegisterModulesCreatedByNeovim"
            });
            if !is_nvim { continue; }
            result.method_found = true;
            for cap in m.captures {
                if Some(cap.index) == idx_lexpr {
                    result.method_last_expr = Some(cap.node);
                }
            }
        }
    }

    // Query 2: AddRange + initializer
    {
        let q = Query::new(&language, r#"
( compilation_unit
  ( class_declaration
    ( base_list ( identifier ) @class.base )
    body: ( declaration_list
      ( method_declaration
        name: ( identifier ) @method.def
        body: ( block
          ( expression_statement
            ( invocation_expression
              function: ( member_access_expression ) @add.range
              arguments: ( argument_list
                ( argument
                  ( array_creation_expression
                    ( initializer_expression
                      ( string_literal )* @last.module .
                    ) @init.expr
                  )
                )
              )
            )
          )
        )
      )
    )
  )
)
"#)?;
        let cap_names = q.capture_names();
        let idx_base = cap_names.iter().position(|n| *n == "class.base").map(|i| i as u32);
        let idx_mdef = cap_names.iter().position(|n| *n == "method.def").map(|i| i as u32);
        let idx_add  = cap_names.iter().position(|n| *n == "add.range").map(|i| i as u32);
        let idx_init = cap_names.iter().position(|n| *n == "init.expr").map(|i| i as u32);
        let idx_lmod = cap_names.iter().position(|n| *n == "last.module").map(|i| i as u32);
        let mut cursor = QueryCursor::new();
        let mut ms = cursor.matches(&q, root_s, source);
        while let Some(m) = ms.next() {
            let is_target = m.captures.iter().any(|c| {
                Some(c.index) == idx_base && node_text(c.node, source) == "TargetRules"
            });
            if !is_target { continue; }
            let is_nvim = m.captures.iter().any(|c| {
                Some(c.index) == idx_mdef
                    && node_text(c.node, source) == "RegisterModulesCreatedByNeovim"
            });
            if !is_nvim { continue; }
            let is_add = m.captures.iter().any(|c| {
                Some(c.index) == idx_add
                    && node_text(c.node, source) == "ExtraModuleNames.AddRange"
            });
            if !is_add { continue; }
            result.add_range_found = true;
            for cap in m.captures {
                if Some(cap.index) == idx_init { result.initializer_expr = Some(cap.node); }
                if Some(cap.index) == idx_lmod { result.last_module = Some(cap.node); }
            }
        }
    }

    // Query 3: module_found + last_decl
    {
        let q = Query::new(&language, r#"
( compilation_unit
  ( class_declaration
    ( base_list ( identifier ) @class.base )
    body: ( declaration_list
      [( method_declaration
        name: ( identifier ) @method.def
        body: ( block
          ( expression_statement
            ( invocation_expression
              function: ( member_access_expression ) @add.range
              arguments: ( argument_list
                ( argument
                  ( array_creation_expression
                    ( initializer_expression
                      ( string_literal
                        ( string_literal_content ) @module.found
                      )
                    )
                  )
                )
              )
            )
          )
        )
      )
      (_)]* @last.decl .
    )
  )
)
"#)?;
        let cap_names = q.capture_names();
        let idx_base   = cap_names.iter().position(|n| *n == "class.base").map(|i| i as u32);
        let idx_mdef   = cap_names.iter().position(|n| *n == "method.def").map(|i| i as u32);
        let idx_mfound = cap_names.iter().position(|n| *n == "module.found").map(|i| i as u32);
        let idx_ldecl  = cap_names.iter().position(|n| *n == "last.decl").map(|i| i as u32);
        let mut cursor = QueryCursor::new();
        let mut ms = cursor.matches(&q, root_s, source);
        while let Some(m) = ms.next() {
            for cap in m.captures {
                if Some(cap.index) == idx_ldecl {
                    result.last_decl = Some(cap.node);
                }
            }
            let is_target = m.captures.iter().any(|c| {
                Some(c.index) == idx_base && node_text(c.node, source) == "TargetRules"
            });
            if !is_target { continue; }
            let is_nvim = m.captures.iter().any(|c| {
                Some(c.index) == idx_mdef
                    && node_text(c.node, source) == "RegisterModulesCreatedByNeovim"
            });
            if !is_nvim { continue; }
            for cap in m.captures {
                if Some(cap.index) == idx_mfound
                    && node_text(cap.node, source) == module_name
                {
                    result.module_found = true;
                }
            }
        }
    }

    Ok(result)
}