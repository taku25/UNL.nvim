-- lua/UNL/parser/cpp.lua
local M = {}
local Query = require("UNL.parser.query")

local function get_node_text(node, bufnr)
    if not node then return nil end
    return vim.treesitter.get_node_text(node, bufnr)
end

-- ★ [New] ボディを持っているかチェック
local function has_body(node)
    if not node then return false end
    for child in node:iter_children() do
        local type = child:type()
        -- field_declaration_list を持っていれば定義とみなす
        if type == "field_declaration_list" then
            return true
        end
    end
    return false
end

function M.parse_header(file_path)
    if not file_path or file_path == "" or vim.fn.filereadable(file_path) == 0 then return {} end

    local bufnr = vim.fn.bufadd(file_path)
    if not vim.api.nvim_buf_is_loaded(bufnr) then
        vim.fn.bufload(bufnr)
        vim.bo[bufnr].filetype = "cpp"
    end

    local ok, parser = pcall(vim.treesitter.get_parser, bufnr, "cpp")
    if not ok or not parser then return {} end

    local tree_root = parser:parse()[1]:root()
    local query = vim.treesitter.query.parse("cpp", Query.cpp_structure)

    local classes = {}
    local current_class = nil
    local current_access = "private"

    -- Helper: create_class (end_line を追加)
    local function create_class(name, kind, line, end_line)
        return {
            name = name,
            kind = kind,
            line = line,
            end_line = end_line, -- ★ 追加
            methods = {},
            fields = {},
            base_class = nil,
        }
    end

    for id, node, _ in query:iter_captures(tree_root, bufnr, 0, -1) do
        local capture_name = query.captures[id]
        local text = get_node_text(node, bufnr)
        
        -- 行情報の取得
        local start_row, _, end_row, _ = node:range()
        local line_num = start_row + 1
        local end_line_num = end_row + 1

        -- 1. クラス定義
        if capture_name == "class_name" or capture_name == "struct_name" then
            local parent = node:parent()
            
            -- ★ [Fix] ボディチェック: 定義本体がない（前方宣言や型参照）ならスキップ
            if not has_body(parent) then
                -- ただし DECLARE_CLASS 等で補完される可能性も考慮し、
                -- struct_specifier 等の親ノードである場合のみ厳密チェック
                local pt = parent:type()
                if pt == "class_specifier" or pt == "struct_specifier" or pt:match("unreal_.*_declaration") then
                     goto continue
                end
            end

            local kind = "class"
            local p_type = parent:type()
            if p_type == "struct_specifier" then kind = "struct" end
            if p_type == "unreal_class_declaration" then kind = "uclass" end
            if p_type == "unreal_struct_declaration" then kind = "ustruct" end
            
            -- 親ノードの範囲を取得 (クラス全体の範囲)
            local p_start, _, p_end, _ = parent:range()
            
            current_class = create_class(text, kind, p_start + 1, p_end + 1)
            
            -- 親クラス取得
            for child in parent:iter_children() do
                if child:type() == "base_class_clause" then
                    for i = 0, child:named_child_count() - 1 do
                        local base_node = child:named_child(i)
                        local base_type = base_node:type()
                        if base_type ~= "access_specifier" and base_type ~= "virtual" then
                            current_class.base_class = get_node_text(base_node, bufnr)
                            break 
                        end
                    end
                end
            end

            if kind == "struct" or kind == "ustruct" then current_access = "public" else current_access = "private" end
            table.insert(classes, current_class)
        
        -- 2. DECLARE_CLASS
        elseif capture_name == "declare_class_macro" then
            local macro_text = get_node_text(node, bufnr)
            local class_name, base_name = macro_text:match("DECLARE_CLASS%s*%(%s*([%w_]+)%s*,%s*([%w_]+)")
            if class_name then
                local final_base = (class_name == base_name or base_name == "None") and nil or base_name
                
                -- 既存のクラス定義内にある場合 (行番号で判定)
                local merged = false
                if current_class then
                    -- 現在解析中のクラスの範囲内か？
                    if line_num >= current_class.line and line_num <= current_class.end_line then
                        if current_class.name == class_name then
                            current_class.base_class = final_base
                            merged = true
                        end
                    end
                end
                
                if not merged then
                    -- 単独定義の場合 (範囲はマクロの行のみ)
                    current_class = create_class(class_name, "intrinsic", line_num, end_line_num)
                    current_class.base_class = final_base
                    current_access = "public"
                    table.insert(classes, current_class)
                end
            end

        -- (access_label, func_name 処理は変更なし... 省略)
        -- ただし func_name の追加時に current_class が nil でないかチェックは既存通り必要
        elseif capture_name == "access_label" then
            if current_class then current_access = text:gsub(":", "") end
        elseif capture_name == "func_name" then
            if current_class then
               -- (... 既存のメソッド解析ロジック ...)
               -- ここは変更不要です
               -- コピペ用:
                local decl_node = node
                local is_valid_method = false
                local p = node:parent()
                for _ = 1, 3 do
                    if not p then break end
                    local pt = p:type()
                    if pt == "field_declaration" or pt == "function_definition" then
                        decl_node = p; is_valid_method = true; break
                    end
                    p = p:parent()
                end
                if is_valid_method then
                    -- virtual再帰探索などは既存のまま
                    local is_virtual = false
                    local function check_virt(n)
                        if not n then return end
                        local t = n:type(); local txt = get_node_text(n, bufnr)
                        if t=="virtual_specifier" or t=="virtual" or txt=="virtual" then is_virtual=true return end
                        if t=="storage_class_specifier" and txt:find("virtual") then is_virtual=true return end
                        for c in n:iter_children() do check_virt(c) if is_virtual then return end end
                    end
                    check_virt(decl_node)

                    local return_type = "void"
                    for child in decl_node:iter_children() do
                        local ct = child:type()
                        if ct:match("type") then return_type = get_node_text(child, bufnr); break end
                    end
                    
                    local params = "()"
                    local func_decl_node = node:parent()
                    while func_decl_node and func_decl_node:type() ~= "function_declarator" do
                        func_decl_node = func_decl_node:parent()
                        if func_decl_node == decl_node then break end
                    end
                    if func_decl_node and func_decl_node:type() == "function_declarator" then
                         for child in func_decl_node:iter_children() do
                            if child:type() == "parameter_list" then params = get_node_text(child, bufnr):gsub("%s+", " "); break end
                        end
                    end
                    table.insert(current_class.methods, { name=text, access=current_access, is_virtual=is_virtual, return_type=return_type, params=params, line=line_num })
                end
            end
        end
        
        ::continue::
    end
    return classes
end
return M
