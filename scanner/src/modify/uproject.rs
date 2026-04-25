//! Replicates the logic of `UEP.nvim/lua/UEP/parser/uproject.lua`.
//!
//! Parses a `.uproject` / `.uplugin` JSON file using tree-sitter-json,
//! then inserts a new Modules entry if the named module is not already listed.

use anyhow::{anyhow, Result};
use tree_sitter::{Node, Parser, Query, QueryCursor};
use streaming_iterator::StreamingIterator;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Add a module entry to `file_path`.
/// Mirrors `M.add_module(file_path, module_opts)` in uproject.lua.
pub fn add_module(
    file_path: &str,
    module_name: &str,
    module_type: &str,
    loading_phase: &str,
) -> Result<()> {
    let raw = std::fs::read_to_string(file_path)?;
    let mut lines: Vec<String> = raw.lines().map(|l| l.to_string()).collect();

    let language: tree_sitter::Language = tree_sitter_json::LANGUAGE.into();
    let mut parser = Parser::new();
    parser.set_language(&language)?;

    let source = raw.as_bytes();
    let tree = parser.parse(source, None).ok_or_else(|| anyhow!("tree-sitter failed to parse"))?;
    let root = tree.root_node();

    // Validate: root must be "document" with first child "object"
    if root.kind() != "document" {
        return Err(anyhow!("Not a valid JSON document"));
    }
    let obj_node = root.child(0).ok_or_else(|| anyhow!("Empty document"))?;
    if obj_node.kind() != "object" {
        return Err(anyhow!("Root is not a JSON object"));
    }

    // ---------------------------------------------------------------------------
    // 4-pattern query (mirrors the Lua query in modules_query())
    // Pattern 0: Capture "Modules" key value node
    // Pattern 1: Check if module with given name already exists
    // Pattern 2: Capture the last object in the Modules array
    // Pattern 3: Capture the last pair in the root object (for indentation)
    // ---------------------------------------------------------------------------
    let query_src = r#"
( document
  ( object
    ( pair
      key: ( string ( string_content ) @obj.key )
      value: (_) @obj.value
    )
  )
)
( document
  ( object
    ( pair
      key: ( string ( string_content ) @mod_key )
      value: ( array
        ( object
          ( pair
            key: ( string ( string_content ) @name_key )
            value: ( string ( string_content ) @name_value )
          )
        )
      )
    )
  )
)
( document
  ( object
    ( pair
      key: ( string ( string_content ) @last_mod_key )
      value: ( array ( object ) @last.module . )
    )
  )
)
( document
  ( object
    ( pair ) @last.entry .
  )
)
"#.to_string();

    let query = Query::new(&language, &query_src)?;
    let cap_names = query.capture_names();

    // Helper: find capture index by name
    let find_cap = |name: &str| -> Option<u32> {
        cap_names.iter().position(|n| *n == name).map(|i| i as u32)
    };
    let idx_obj_key    = find_cap("obj.key");
    let idx_obj_value  = find_cap("obj.value");
    let idx_mod_key    = find_cap("mod_key");
    let idx_name_key   = find_cap("name_key");
    let idx_name_value = find_cap("name_value");
    let idx_last_mod_key = find_cap("last_mod_key");
    let idx_last_mod   = find_cap("last.module");
    let idx_last_entry = find_cap("last.entry");

    let mut modules_node: Option<Node> = None;
    let mut module_exists = false;
    let mut last_node: Option<Node> = None;
    let mut last_entry: Option<Node> = None;
    let mut tabstop = String::new();

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root, source);

    while let Some(m) = matches.next() {
        let pattern = m.pattern_index;
        let captures = m.captures;

        match pattern {
            0 => {
                // Pattern 0: obj.key == "Modules" → capture obj.value
                let has_modules = captures.iter().any(|c| {
                    Some(c.index) == idx_obj_key
                        && node_text(c.node, source) == "Modules"
                });
                if has_modules {
                    for c in captures {
                        if Some(c.index) == idx_obj_value {
                            modules_node = Some(c.node);
                        }
                    }
                }
            }
            1 => {
                // Pattern 1: check if named module exists
                let has_modules = captures.iter().any(|c| {
                    Some(c.index) == idx_mod_key
                        && node_text(c.node, source) == "Modules"
                });
                let has_name = captures.iter().any(|c| {
                    Some(c.index) == idx_name_key
                        && node_text(c.node, source) == "Name"
                });
                if has_modules && has_name {
                    for c in captures {
                        if Some(c.index) == idx_name_value
                            && node_text(c.node, source) == module_name
                        {
                            module_exists = true;
                        }
                    }
                }
            }
            2 => {
                // Pattern 2: last object in Modules array
                let has_modules = captures.iter().any(|c| {
                    Some(c.index) == idx_last_mod_key
                        && node_text(c.node, source) == "Modules"
                });
                if has_modules {
                    for c in captures {
                        if Some(c.index) == idx_last_mod {
                            last_node = Some(c.node);
                        }
                    }
                }
            }
            3 => {
                // Pattern 3: last pair in root → detect indentation
                for c in captures {
                    if Some(c.index) == idx_last_entry {
                        let row = c.node.start_position().row;
                        let col = c.node.start_position().column;
                        tabstop = lines[row][..col].to_string();
                        last_entry = Some(c.node);
                    }
                }
            }
            _ => {}
        }
    }

    if module_exists {
        return Ok(());
    }

    if modules_node.is_none() {
        // No "Modules" key – insert entire Modules block after last_entry
        let entry = last_entry.ok_or_else(|| anyhow!("Could not find last root entry"))?;
        let end_row = entry.end_position().row;
        let end_col = entry.end_position().column;
        let line_len = lines[end_row].len();

        if line_len != end_col {
            let trailing = lines[end_row][end_col..].to_string();
            lines.insert(end_row + 1, trailing);
        }
        let prefix = lines[end_row][..end_col].to_string();
        lines[end_row] = format!("{},", prefix);

        let t1 = &tabstop;
        let t2 = format!("{}{}", t1, t1);
        let t3 = format!("{}{}{}", t1, t1, t1);
        lines.insert(end_row + 1, format!("{}]", t1));
        lines.insert(end_row + 1, format!("{}}}", t2));
        lines.insert(end_row + 1, format!("{}\"LoadingPhase\": \"{}\"", t3, loading_phase));
        lines.insert(end_row + 1, format!("{}\"Type\": \"{}\",", t3, module_type));
        lines.insert(end_row + 1, format!("{}\"Name\": \"{}\",", t3, module_name));
        lines.insert(end_row + 1, format!("{}{{", t2));
        lines.insert(end_row + 1, format!("{}\"Modules\": [", t1));
    } else {
        // Modules array exists – append new module after last_node
        let node = last_node.ok_or_else(|| anyhow!("Modules array exists but last_node is None"))?;
        let end_row = node.end_position().row;
        let end_col = node.end_position().column;
        let line_len = lines[end_row].len();

        if line_len != end_col {
            let trailing = lines[end_row][end_col..].to_string();
            lines.insert(end_row + 1, trailing);
        }
        let prefix = lines[end_row][..end_col].to_string();
        lines[end_row] = format!("{},", prefix);

        let t1 = &tabstop;
        let t2 = format!("{}{}", t1, t1);
        let t3 = format!("{}{}{}", t1, t1, t1);
        lines.insert(end_row + 1, format!("{}}}", t2));
        lines.insert(end_row + 1, format!("{}\"LoadingPhase\": \"{}\"", t3, loading_phase));
        lines.insert(end_row + 1, format!("{}\"Type\": \"{}\",", t3, module_type));
        lines.insert(end_row + 1, format!("{}\"Name\": \"{}\",", t3, module_name));
        lines.insert(end_row + 1, format!("{}{{", t2));
    }

    let backup = format!("{}.old", file_path);
    std::fs::copy(file_path, &backup)?;
    std::fs::write(file_path, lines.join("\n"))?;
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

