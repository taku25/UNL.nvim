local M = {}

function M.get_registry_path()
    local cache_dir = vim.fn.stdpath("cache") .. "/UNL"
    if vim.fn.isdirectory(cache_dir) == 0 then
        vim.fn.mkdir(cache_dir, "p")
    end
    return cache_dir .. "/registered_projects.json"
end

function M.load()
    local path = M.get_registry_path()
    if vim.fn.filereadable(path) == 0 then return {} end
    
    local ok, lines = pcall(vim.fn.readfile, path)
    if not ok then return {} end
    
    local json = table.concat(lines, "\n")
    local ok_decode, data = pcall(vim.json.decode, json)
    if not ok_decode then return {} end
    
    -- Convert map to list if necessary, but server saves as map?
    -- server_main.rs: projects: HashMap<PathBuf, ProjectContext>
    -- JSON: { "path": { ... }, "path2": { ... } }
    -- list_projects RPC returns a list of objects.
    -- We need to convert the map from JSON to the list format expected by the UI.
    
    local list = {}
    for root, ctx in pairs(data) do
        table.insert(list, {
            root = root,
            db_path = ctx.db_path,
            vcs_hash = ctx.vcs_hash
        })
    end
    return list, data -- Return list and raw map
end

function M.save(data_map)
    local path = M.get_registry_path()
    local encoded = vim.json.encode(data_map)
    vim.fn.writefile({encoded}, path)
end

function M.remove(project_root)
    local list, map = M.load()
    local target_key = nil
    
    -- Find key (normalize slashes just in case)
    -- Server uses PathBuf keys. JSON keys are strings.
    -- We need to match exactly or normalized.
    
    if map[project_root] then
        target_key = project_root
    else
        -- Try normalized matching
        local norm_target = project_root:gsub("\\", "/")
        for k, _ in pairs(map) do
            if k:gsub("\\", "/") == norm_target then
                target_key = k
                break
            end
        end
    end
    
    if target_key then
        map[target_key] = nil
        M.save(map)
        return true
    end
    return false
end

return M
