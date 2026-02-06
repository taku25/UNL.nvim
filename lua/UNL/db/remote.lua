local M = {}
local finder = require("UNL.finder")
local rpc = require("UNL.rpc")
local log = require("UNL.logging").get("UNL")
local server = require("UNL.scanner.server")

local function get_project_root()
    local project_info = finder.project.find_project(vim.loop.cwd())
    if project_info and project_info.uproject then
        local root = vim.fn.fnamemodify(project_info.uproject, ":h")
        return root:gsub("\\", "/")
    end
    return nil
end

function M.request(kind, args, callback)
    local root = get_project_root()
    if not root then 
        if callback then callback(nil, "Project root not found") end
        return 
    end
    
    local params = {
        project_root = root,
        kind = kind,
    }
    for k, v in pairs(args or {}) do params[k] = v end
    
    rpc.request("query", params, nil, function(success, result_or_err)
        if success then
            if callback then callback(result_or_err) end
        else
            log.error("UNL Query error (%s): %s", kind, tostring(result_or_err))
            if callback then callback(nil, result_or_err) end
        end
    end)
end

function M.request_streaming(kind, args, on_partial, on_complete)
    local root = get_project_root()
    if not root then 
        if on_complete then on_complete(false, "Project root not found") end
        return 
    end
    
    local params = { project_root = root, kind = kind }
    for k, v in pairs(args or {}) do params[k] = v end
    
    rpc.request("query", params, function(method, notify_params)
        if method == "query/partial" then
            local count = (notify_params.items and #notify_params.items) or 0
            log.debug("Remote: Partial result received. msgid=%s, count=%d", tostring(notify_params.msgid), count)
            if on_partial then on_partial(notify_params.items) end
        end
    end, function(success, result_or_err)
        if not success then
            log.error("UNL Streaming Query error (%s): %s", kind, tostring(result_or_err))
        end
        if on_complete then on_complete(success, result_or_err) end
    end)
end

-- Async Wrappers

function M.get_files_in_modules_async(modules, extensions, filter, on_partial, on_complete)
    M.request_streaming("GetFilesInModulesAsync", { modules = modules, extensions = extensions, filter = filter }, on_partial, on_complete)
end

function M.search_files_in_modules_async(modules, filter, limit, on_partial, on_complete)
    M.request_streaming("SearchFilesInModulesAsync", { modules = modules, filter = filter, limit = limit }, on_partial, on_complete)
end

function M.get_classes_in_modules_async(modules, symbol_type, on_partial, on_complete)
    M.request_streaming("GetClassesInModulesAsync", { modules = modules, symbol_type = symbol_type }, on_partial, on_complete)
end

-- Standard Wrappers

function M.find_derived_classes(base_class, cb)
    M.request("FindDerivedClasses", { base_class = base_class }, cb)
end

function M.search_files(part, cb)
    M.request("SearchFiles", { part = part }, cb)
end

function M.load_component_data(component, cb)
    M.request("LoadComponentData", { component = component }, cb)
end

function M.get_module_by_name(name, cb)
    M.request("GetModuleByName", { name = name }, cb)
end

function M.get_classes_in_modules(modules, symbol_type, cb)
    if type(symbol_type) == "function" then
        cb = symbol_type
        symbol_type = nil
    end
    M.request("GetClassesInModules", { modules = modules, symbol_type = symbol_type }, cb)
end

function M.get_recursive_derived_classes(base_class, cb)
    M.request("GetRecursiveDerivedClasses", { base_class = base_class }, cb)
end

function M.get_recursive_parent_classes(child_class, cb)
    M.request("GetRecursiveParentClasses", { child_class = child_class }, cb)
end

function M.find_symbol_in_inheritance_chain(class_name, symbol_name, mode, cb)
    M.request("FindSymbolInInheritanceChain", { class_name = class_name, symbol_name = symbol_name, mode = mode }, cb)
end

function M.get_virtual_functions_in_inheritance_chain(class_name, cb)
    M.request("GetVirtualFunctionsInInheritanceChain", { class_name = class_name }, cb)
end

function M.get_program_files(cb)
    M.request("GetProgramFiles", {}, cb)
end

function M.get_all_ini_files(cb)
    M.request("GetAllIniFiles", {}, cb)
end

function M.find_symbol_in_module(module, symbol, cb)
    M.request("FindSymbolInModule", { module = module, symbol = symbol }, cb)
end

function M.find_class_by_name(name, cb)
    M.request("FindClassByName", { name = name }, cb)
end

function M.search_classes_prefix(prefix, limit, cb)
    M.request("SearchClassesPrefix", { prefix = prefix, limit = limit }, cb)
end

function M.get_classes(extra_where, params, cb)
    M.request("GetClasses", { extra_where = extra_where, params = params }, cb)
end

function M.get_structs(extra_where, params, cb)
    M.request("GetStructs", { extra_where = extra_where, params = params }, cb)
end

function M.get_structs_only(cb)
    M.request("GetStructsOnly", {}, cb)
end

function M.get_class_members_by_id(class_id, cb)
    M.request("GetClassMembersById", { class_id = class_id }, cb)
end

function M.get_class_members(class_name, cb)
    M.request("GetClassMembers", { class_name = class_name }, cb)
end

function M.get_class_methods(class_name, cb)
    M.request("GetClassMethods", { class_name = class_name }, cb)
end

function M.get_class_properties(class_name, cb)
    M.request("GetClassProperties", { class_name = class_name }, cb)
end

function M.get_class_members_recursive(class_name, namespace, cb)
    M.request("GetClassMembersRecursive", { class_name = class_name, namespace = namespace }, cb)
end

function M.search_files_by_path_part(part, cb)
    M.request("SearchFilesByPathPart", { part = part }, cb)
end

function M.get_enum_values(enum_name, cb)
    M.request("GetEnumValues", { enum_name = enum_name }, cb)
end

function M.get_components(cb)
    M.request("GetComponents", {}, cb)
end

function M.get_modules(cb)
    M.request("GetModules", {}, cb)
end

function M.get_module_id_by_name(name, cb)
    M.request("GetModuleIdByName", { name = name }, cb)
end

function M.get_module_root_path(name, cb)
    M.request("GetModuleRootPath", { name = name }, cb)
end

function M.get_files_in_module(module_id, cb)
    M.request("GetFilesInModule", { module_id = module_id }, cb)
end

function M.get_files_in_modules(modules, extensions, filter, cb)
    if type(extensions) == "function" then
        cb = extensions
        extensions = nil
        filter = nil
    elseif type(filter) == "function" then
        cb = filter
        filter = nil
    end
    M.request("GetFilesInModules", { modules = modules, extensions = extensions, filter = filter }, cb)
end

function M.search_files_in_modules(modules, filter, limit, cb)
    M.request("SearchFilesInModules", { modules = modules, filter = filter, limit = limit }, cb)
end

function M.search_symbols_in_modules(modules, symbol_type, filter, limit, cb)
    M.request("SearchSymbolsInModules", { modules = modules, symbol_type = symbol_type, filter = filter, limit = limit }, cb)
end

function M.get_directories_in_module(module_id, cb)
    M.request("GetDirectoriesInModule", { module_id = module_id }, cb)
end

function M.get_module_files_by_name_and_root(name, root, cb)
    M.request("GetModuleFilesByNameAndRoot", { name = name, root = root }, cb)
end

function M.get_module_dirs_by_name_and_root(name, root, cb)
    M.request("GetModuleDirsByNameAndRoot", { name = name, root = root }, cb)
end

function M.get_all_files_metadata(cb)
    M.request("GetAllFilesMetadata", {}, cb)
end

function M.get_class_file_path(class_name, cb)
    M.request("GetClassFilePath", { class_name = class_name }, cb)
end

function M.get_file_symbols(file_path, cb)
    M.request("GetFileSymbols", { file_path = file_path }, cb)
end

function M.parse_buffer(bufnr, cb)
    bufnr = bufnr or vim.api.nvim_get_current_buf()
    if not vim.api.nvim_buf_is_valid(bufnr) then
        if cb then cb(nil, "Invalid buffer") end
        return
    end

    local lines = vim.api.nvim_buf_get_lines(bufnr, 0, -1, false)
    local content = table.concat(lines, "\n")
    local file_path = vim.api.nvim_buf_get_name(bufnr)

    M.request("ParseBuffer", {
        content = content,
        file_path = (file_path ~= "") and file_path or nil
    }, cb)
end

function M.update_member_return_type(class_name, member_name, return_type, cb)
    M.request("UpdateMemberReturnType", { class_name = class_name, member_name = member_name, return_type = return_type }, cb)
end

function M.get_target_files(cb)
    M.request("GetTargetFiles", {}, cb)
end

function M.get_all_file_paths(cb)
    M.request("GetAllFilePaths", {}, cb)
end

return M