-- lua/UNL/vcs/git.lua
local unl_path = require("UNL.path")
local M = {}

local git_status_cache = {}
local git_unpushed_cache = {}

local function make_key(path)
    return unl_path.normalize(path)
end

local function spawn_git(args, cwd, on_success)
    local stdout = vim.loop.new_pipe(false)
    local stderr = vim.loop.new_pipe(false)
    local output_data = ""

    local handle, pid
    handle, pid = vim.loop.spawn("git", {
        args = args,
        cwd = cwd,
        stdio = { nil, stdout, stderr }
    }, function(code, signal)
        stdout:read_stop()
        stderr:read_stop()
        stdout:close()
        stderr:close()
        handle:close()

        vim.schedule(function()
            if code == 0 then
                on_success(output_data)
            else
                on_success(nil) 
            end
        end)
    end)

    if handle then
        vim.loop.read_start(stdout, function(err, data)
            if data then output_data = output_data .. data end
        end)
        vim.loop.read_start(stderr, function(err, data) end)
    else
        vim.schedule(function() on_success(nil) end)
    end
end

local function parse_status_output(base_path, output_str, cache_table)
    if not output_str or output_str == "" then return end
    for line in output_str:gmatch("[^\r\n]+") do
        if #line > 3 then
            local status = line:sub(1, 2):gsub("%s", "")
            local rel_path = line:sub(4)
            if rel_path:sub(1, 1) == '"' then rel_path = rel_path:sub(2, -2) end
            local abs_path = base_path .. "/" .. rel_path
            local key = make_key(abs_path)
            if key then cache_table[key] = status end
        end
    end
end

local function parse_unpushed_output(base_path, output_str, cache_list)
    if not output_str then return end
    for line in output_str:gmatch("[^\r\n]+") do
        if line ~= "" then
            local rel_path = line
            if rel_path:sub(1, 1) == '"' then rel_path = rel_path:sub(2, -2) end
            local abs_path = base_path .. "/" .. rel_path
            local key = make_key(abs_path)
            if key then 
                local exists = false
                for _, v in ipairs(cache_list) do if v.path == key then exists = true break end end
                if not exists then
                    table.insert(cache_list, { path = key, status = "Unpushed" })
                end
            end
        end
    end
end

function M.get_hash(root)
    local output = vim.fn.systemlist("git -C " .. vim.fn.shellescape(root) .. " rev-parse HEAD")
    if vim.v.shell_error == 0 and #output > 0 then
        return "git:" .. output[1]
    end
    return nil
end

--- 2つのコミット間の変更ファイルリストを非同期で取得する
--- @param root string Git リポジトリのルートパス
--- @param old_hash string 前回のコミットハッシュ（"git:" プレフィックス付きも可）
--- @param new_hash string 現在のコミットハッシュ（"git:" プレフィックス付きも可）
--- @param callback function(files: string[]|nil) 変更ファイルの絶対パスリスト
function M.get_changed_files(root, old_hash, new_hash, callback)
    if not root or not old_hash or not new_hash then
        return callback(nil)
    end
    -- "git:abc123" → "abc123"
    local old_ref = old_hash:gsub("^git:", "")
    local new_ref = new_hash:gsub("^git:", "")

    spawn_git({ "diff", "--name-only", "--diff-filter=ACMR", old_ref, new_ref }, root, function(output)
        if not output then return callback(nil) end
        local root_norm = unl_path.normalize(root)
        local files = {}
        for line in output:gmatch("[^\r\n]+") do
            if line ~= "" then
                table.insert(files, root_norm .. "/" .. line)
            end
        end
        callback(files)
    end)
end

function M.refresh(start_path, on_complete, logger_name)
    if not start_path then return end

    local git_dir = vim.fn.finddir(".git", start_path .. ";")
    local git_file = vim.fn.findfile(".git", start_path .. ";")

    if git_dir == "" and git_file == "" then
        if on_complete then on_complete() end
        return
    end

    spawn_git({"rev-parse", "--show-toplevel"}, start_path, function(output)
        local git_root = output and output:gsub("[\r\n]+", "") or ""
        
        if git_root == "" then
            if on_complete then on_complete() end
            return
        end

        git_root = unl_path.normalize(git_root)
        
        local pending_jobs = 0
        local new_status_cache = {}
        local new_unpushed_cache = {}
        local is_finished = false

        local function check_done()
            pending_jobs = pending_jobs - 1
            if pending_jobs <= 0 and not is_finished then
                is_finished = true
                git_status_cache = new_status_cache
                git_unpushed_cache = new_unpushed_cache
                if on_complete then on_complete() end
            end
        end
        
        local function add_job() pending_jobs = pending_jobs + 1 end

        -- Job 1: Local Changes
        add_job()
        spawn_git({"status", "--porcelain", "-u", "--no-renames"}, git_root, function(root_stat)
            parse_status_output(git_root, root_stat, new_status_cache)
            check_done()
        end)

        -- Job 2: Unpushed Commits (Remote Diff)
        add_job()
        spawn_git({"diff", "--name-only", "--diff-filter=ACMR", "@{u}...HEAD"}, git_root, function(unpushed_out)
            parse_unpushed_output(git_root, unpushed_out, new_unpushed_cache)
            check_done()
        end)

        -- Job 3: Submodules
        add_job()
        spawn_git({"submodule", "status", "--recursive"}, git_root, function(sub_out)
            if sub_out and sub_out ~= "" then
                for line in sub_out:gmatch("[^\r\n]+") do
                    local clean_line = line:match("^%W*(.+)$")
                    if clean_line then
                        local parts = {}
                        for p in clean_line:gmatch("%S+") do table.insert(parts, p) end
                        if #parts >= 2 then
                            local sub_rel_path = parts[2]
                            local sub_abs_path = git_root .. "/" .. sub_rel_path
                            sub_abs_path = unl_path.normalize(sub_abs_path)
                            
                            add_job()
                            spawn_git({"status", "--porcelain", "-u", "--no-renames"}, sub_abs_path, function(sub_stat)
                                parse_status_output(sub_abs_path, sub_stat, new_status_cache)
                                check_done()
                            end)
                        end
                    end
                end
            end
            check_done()
        end)
    end)
end

function M.get_status(path)
    if not path then return nil end
    local key = make_key(path)
    return git_status_cache[key]
end

function M.clear()
    git_status_cache = {}
    git_unpushed_cache = {}
end

function M.get_changes()
    local changes = {}
    for path, status in pairs(git_status_cache) do
        if status ~= "!!" then
            table.insert(changes, { path = path, status = status })
        end
    end
    return changes
end

function M.get_unpushed()
    return git_unpushed_cache or {}
end

function M.get_file_content(path, on_success)
    if not path then return on_success(nil) end
    
    local git_dir = vim.fn.finddir(".git", path .. ";")
    local git_file = vim.fn.findfile(".git", path .. ";")
    
    if git_dir == "" and git_file == "" then
        return on_success(nil)
    end

    spawn_git({"rev-parse", "--show-toplevel"}, vim.fn.fnamemodify(path, ":h"), function(git_root)
        if not git_root then return on_success(nil) end
        git_root = git_root:gsub("[\r\n]+", "")
        local root_norm = unl_path.normalize(git_root)
        local path_norm = unl_path.normalize(path)
        local rel_path = path_norm:sub(#root_norm + 2)
        spawn_git({"show", "HEAD:" .. rel_path}, git_root, on_success)
    end)
end

return M
