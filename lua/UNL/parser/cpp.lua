-- lua/UNL/parser/cpp.lua
local M = {}
local Query = require("UNL.parser.query")

--- テキスト取得ヘルパー
local function get_node_text(node, bufnr)
    if not node then return nil end
    return vim.treesitter.get_node_text(node, bufnr)
end

--- ファイルを解析してクラス構造データを返す
--- @param file_path string 解析対象のファイルパス
--- @return table classes クラス情報のリスト
function M.parse_header(file_path)
    if not file_path or file_path == "" or vim.fn.filereadable(file_path) == 0 then 
        return {} 
    end

    -- バッファ読み込み
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

    -- クラスデータ作成ヘルパー
    local function create_class(name, kind, line)
        return {
            name = name,
            kind = kind, 
            line = line,
            methods = {}, 
            fields = {},
            base_class = nil,
        }
    end

    for id, node, _ in query:iter_captures(tree_root, bufnr, 0, -1) do
        local capture_name = query.captures[id]
        local text = get_node_text(node, bufnr)
        local row, _, _, _ = node:range()
        local line_num = row + 1

        -- 1. クラス定義
        if capture_name == "class_name" or capture_name == "struct_name" then
            local parent = node:parent()
            local kind = "class"
            local p_type = parent:type()
            
            if p_type == "struct_specifier" then kind = "struct" end
            if p_type == "unreal_class_declaration" then kind = "uclass" end
            if p_type == "unreal_struct_declaration" then kind = "ustruct" end
            
            current_class = create_class(text, kind, line_num)
            
            -- ▼▼▼ 修正: 親クラスの取得ロジック ▼▼▼
            for child in parent:iter_children() do
                if child:type() == "base_class_clause" then
                    -- base_class_clause の子要素を走査し、アクセス指定子以外を取得する
                    for i = 0, child:named_child_count() - 1 do
                        local base_node = child:named_child(i)
                        local base_type = base_node:type()
                        
                        -- 'access_specifier' や 'virtual' ではないノードがクラス名
                        if base_type ~= "access_specifier" and base_type ~= "virtual" then
                            current_class.base_class = get_node_text(base_node, bufnr)
                            break -- 最初の基底クラスが見つかれば終了 (多重継承は未対応だがUEでは稀)
                        end
                    end
                end
            end
            -- ▲▲▲ 修正ここまで ▲▲▲

            if kind == "struct" or kind == "ustruct" then current_access = "public" else current_access = "private" end
            table.insert(classes, current_class)
        
        -- 2. DECLARE_CLASS マクロ (UObject用)
        elseif capture_name == "declare_class_macro" then
            local macro_text = get_node_text(node, bufnr)
            local class_name, base_name = macro_text:match("DECLARE_CLASS%s*%(%s*([%w_]+)%s*,%s*([%w_]+)")
            
            if class_name then
                local final_base = base_name
                if class_name == base_name or base_name == "None" then
                    final_base = nil
                end
                
                if current_class and current_class.name == class_name then
                    current_class.base_class = final_base
                else
                    current_class = create_class(class_name, "intrinsic", line_num)
                    current_class.base_class = final_base
                    current_access = "public"
                    table.insert(classes, current_class)
                end
            end

        -- 3. アクセス指定子
        elseif capture_name == "access_label" then
            if current_class then
                current_access = text:gsub(":", "")
            end

        -- 4. メソッド定義
        elseif capture_name == "func_name" then
            if current_class then
                local decl_node = node
                while decl_node and decl_node:type() ~= "field_declaration" do
                    decl_node = decl_node:parent()
                end
                
                local is_virtual = false
                local return_type = "void"
                
                if decl_node then
                    for child in decl_node:iter_children() do
                        local child_text = get_node_text(child, bufnr)
                        local child_type = child:type()

                        if child_text == "virtual" or child_type == "virtual_specifier" then
                            is_virtual = true
                        elseif child_type == "primitive_type" or child_type == "type_identifier" or child_type == "template_type" then
                            return_type = child_text
                        end
                    end
                end

                local params = "()" 
                local parent = node:parent()
                if parent then
                    for child in parent:iter_children() do
                        if child:type() == "parameter_list" then
                            params = get_node_text(child, bufnr):gsub("%s+", " ")
                            break
                        end
                    end
                end
                
                table.insert(current_class.methods, {
                    name = text,
                    access = current_access,
                    is_virtual = is_virtual,
                    return_type = return_type,
                    params = params,
                    line = line_num
                })
            end
        end
    end

    return classes
end

return M
