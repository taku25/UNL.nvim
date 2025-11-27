-- lua/UNL/parser/cpp.lua
local M = {}
local Query = require("UNL.parser.query")

local function get_logger()
    local ok_uep, uep_log = pcall(require, "UEP.logger")
    if ok_uep then return uep_log.get() end
    local ok_unx, unx_log = pcall(require, "UNX.logger")
    if ok_unx then return unx_log.get() end
    return { debug = function() end, info = function() end, warn = function() end, error = function() end }
end
local logger = get_logger()

local function get_node_text(node, bufnr)
    if not node then return nil end
    local text = vim.treesitter.get_node_text(node, bufnr)
    if not text then return "" end
    return text:gsub("%s+", " ")
end

local function has_child_type(node, type_name)
    for child in node:iter_children() do
        if child:type() == type_name then return true end
    end
    return false
end

-- ボディ判定 (書きかけのコードに対応)
local function has_body(node, bufnr)
    if not node then return false end
    for child in node:iter_children() do
        if child:type() == "field_declaration_list" then return true end
        if child:type() == "enumerator_list" then return true end
        if child:type() == "ERROR" then goto text_check end
    end
    ::text_check::
    local text = vim.treesitter.get_node_text(node, bufnr)
    if text:find("GENERATED_BODY") or text:find("GENERATED_UCLASS_BODY") or text:find("{") then
        return true
    end
    return false
end

local function get_parameters_text(node, bufnr)
    local p = node
    for _ = 1, 5 do
        if not p then break end
        if p:type() == "function_declarator" then
            for child in p:iter_children() do
                if child:type() == "parameter_list" then
                    return get_node_text(child, bufnr)
                end
            end
        end
        p = p:parent()
    end
    return "()"
end

local function create_class_data(name, kind, line, end_line, file_path)
    local default_access = (kind == "Struct" or kind == "UStruct") and "public" or "private"
    return {
        name = name, kind = kind, line = line, end_line = end_line, file_path = file_path,
        base_class = nil, current_access = default_access,
        methods = { public = {}, protected = {}, private = {}, impl = {} },
        fields  = { public = {}, protected = {}, private = {}, impl = {} }
    }
end

local function create_global_data()
    return { methods = {}, fields = {} }
end

function M.parse(path_or_bufnr)
    local result = { list = {}, map = {}, globals = create_global_data() }
    local bufnr
    local file_path = ""
    local should_delete_buffer = false

    -- 引数判定
    if type(path_or_bufnr) == "number" then
        bufnr = path_or_bufnr
        if not vim.api.nvim_buf_is_valid(bufnr) then return result end
        file_path = vim.api.nvim_buf_get_name(bufnr)
    else
        file_path = path_or_bufnr
        if not file_path or file_path == "" or vim.fn.filereadable(file_path) == 0 then return result end
        bufnr = vim.fn.bufadd(file_path)
        if not vim.api.nvim_buf_is_loaded(bufnr) then
            vim.fn.bufload(bufnr)
            -- ★修正: ここで filetype = "cpp" を設定しない！
            -- これにより LSP が反応してワーニングを出すのを防ぐ
            should_delete_buffer = true
        end
    end

    -- ★修正: get_parser に明示的に "cpp" を渡すことで、filetype設定なしでもパース可能にする
    local ok, parser = pcall(vim.treesitter.get_parser, bufnr, "cpp")
    if not ok or not parser then 
        if should_delete_buffer and vim.api.nvim_buf_is_valid(bufnr) then
            vim.api.nvim_buf_delete(bufnr, { force = true })
        end
        return result 
    end

    local tree = parser:parse(true)[1]
    if not tree then 
        if should_delete_buffer and vim.api.nvim_buf_is_valid(bufnr) then
            vim.api.nvim_buf_delete(bufnr, { force = true })
        end
        return result 
    end
    local tree_root = tree:root()
    
    local query = vim.treesitter.query.parse("cpp", Query.cpp_structure)
    local current_class = nil
    local pending_impl_class = nil

    for id, node, _ in query:iter_captures(tree_root, bufnr, 0, -1) do
        local capture_name = query.captures[id]
        if capture_name:match("_def$") then goto continue end

        local text = get_node_text(node, bufnr) 
        local s_row, _, e_row, _ = node:range()
        local line_num = s_row + 1
        
        -- クラス定義ノードを探す（さかのぼる）
        local definition_node = node
        while definition_node do
            local type = definition_node:type()
            if type:match("declaration") or type:match("specifier") or type:match("definition") then break end
            definition_node = definition_node:parent()
        end
        if not definition_node then definition_node = node end

        if capture_name == "class_name" or capture_name == "struct_name" or capture_name == "enum_name" then
            if not has_body(definition_node, bufnr) then goto continue end

            local kind = "Class"
            if capture_name == "struct_name" then kind = "Struct" end
            if capture_name == "enum_name" then kind = "Enum" end
            local type = definition_node:type()
            if type == "unreal_class_declaration" then kind = "UClass" end
            if type == "unreal_struct_declaration" then kind = "UStruct" end
            if type == "unreal_enum_declaration" then kind = "UEnum" end

            local base_class = nil
            -- 基底クラス名を抽出 (public/virtual除外)
            for child in definition_node:iter_children() do
                if child:type() == "base_class_clause" then
                    for i = 0, child:named_child_count() - 1 do
                        local base_node = child:named_child(i)
                        local btype = base_node:type()
                        if btype ~= "access_specifier" and btype ~= "virtual" then
                            base_class = get_node_text(base_node, bufnr)
                            break 
                        end
                    end
                end
            end

            -- ★修正: クラス定義全体(definition_node)の範囲を使用する
            local d_start, _, d_end, _ = definition_node:range()
            local def_start_line = d_start + 1
            local def_end_line = d_end + 1

            local new_class = create_class_data(text, kind, def_start_line, def_end_line, file_path)
            new_class.base_class = base_class
            table.insert(result.list, new_class)
            result.map[text] = new_class
            current_class = new_class

        elseif capture_name == "declare_class_macro" then
            local class_name, base_name = text:match("DECLARE_CLASS%s*%(%s*([%w_]+)%s*,%s*([%w_]+)")
            if class_name then
                local final_base = (class_name == base_name or base_name == "None") and nil or base_name
                if current_class and current_class.name == class_name then
                    current_class.base_class = final_base
                else
                    -- DECLARE_CLASSはマクロ自体が範囲
                    local d_start, _, d_end, _ = node:range()
                    local new_cls = create_class_data(class_name, "Intrinsic", d_start + 1, d_end + 1, file_path)
                    new_cls.base_class = final_base
                    new_cls.current_access = "public"
                    table.insert(result.list, new_cls)
                    result.map[class_name] = new_cls
                    current_class = new_cls
                end
            end

        elseif capture_name == "impl_class" then
            pending_impl_class = text

        elseif capture_name == "access_label" then
            if current_class and line_num >= current_class.line and line_num <= current_class.end_line then
                current_class.current_access = text:gsub(":", ""):gsub("%s+", "")
            end

        elseif capture_name == "func_name" then
            local kind = "Function"
            if has_child_type(definition_node, "ufunction_macro") then kind = "UFunction" end
            local target_class = current_class
            local access_bucket = target_class and target_class.current_access or "public"

            if pending_impl_class then
                target_class = result.map[pending_impl_class] or create_class_data(pending_impl_class, "Class", 0, 0, file_path)
                result.map[pending_impl_class] = target_class
                access_bucket = "impl"; kind = "Implementation"
                if text == pending_impl_class or text == "~" .. pending_impl_class then kind = "Constructor" end
                pending_impl_class = nil
            elseif target_class then
                if text == target_class.name or text == "~" .. target_class.name then kind = "Constructor" end
            else
                target_class = nil
            end

            local params = get_parameters_text(node, bufnr)
            local item = {
                name = text, detail = params, kind = kind, line = line_num, file_path = file_path,
                params = params, is_virtual = false
            }
            
            local func_full_text = get_node_text(definition_node, bufnr)
            if func_full_text and func_full_text:match("virtual") then item.is_virtual = true end
            if item.is_virtual then
                local ret = func_full_text:match("virtual%s+([%w_:]+[%*%&]?)%s+" .. text)
                if ret then item.return_type = ret end
            else
                local ret = func_full_text:match("^%s*([%w_:]+[%*%&]?)%s+" .. text)
                if ret and ret ~= "virtual" then item.return_type = ret end
            end

            if target_class then table.insert(target_class.methods[access_bucket], item)
            else table.insert(result.globals.methods, item) end

        elseif capture_name == "field_name" then
            local kind = "Field"
            if has_child_type(definition_node, "uproperty_macro") then kind = "UProperty" end
            local item = { name = text, kind = kind, line = line_num, file_path = file_path }
            if current_class and line_num >= current_class.line and line_num <= current_class.end_line then
                table.insert(current_class.fields[current_class.current_access], item)
            else
                table.insert(result.globals.fields, item)
            end
        end
        ::continue::
    end

    if should_delete_buffer and vim.api.nvim_buf_is_valid(bufnr) then
        vim.api.nvim_buf_delete(bufnr, { force = true })
    end
    
    return result
end

function M.find_best_match_class(parse_result, target_name)
    local map = parse_result.map
    local list = parse_result.list
    local class_data = map[target_name]
    if not class_data then
        for name, data in pairs(map) do
            if name:lower() == target_name:lower() then class_data = data; break end
        end
    end
    if not class_data then
        local short_target = target_name:match("^[UAFE](.*)") or target_name
        for name, data in pairs(map) do
             local short_name = name:match("^[UAFE](.*)") or name
             if short_name == short_target then class_data = data; break end
        end
    end
    if not class_data and #list > 0 then
        table.sort(list, function(a,b) 
             local function count_members(c)
                local m = #c.methods.public + #c.methods.protected + #c.methods.private
                local f = #c.fields.public + #c.fields.protected + #c.fields.private
                return m + f
             end
             return count_members(a) > count_members(b) 
        end)
        class_data = list[1]
    end
    return class_data
end

return M
