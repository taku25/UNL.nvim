-- lua/UNL/vcs/p4.lua
local unl_path = require("UNL.path")
local M = {}

-- キャッシュ: [正規化されたパス] = "ステータスコード" (例: "edit", "add", "delete")
local p4_status_cache = {}
-- P4が使えるかどうかのフラグ (ディレクトリごとにキャッシュ)
local availability_cache = {}

-- キャッシュキー生成
local function make_key(path)
    return unl_path.normalize(path)
end

-- 非同期 P4 コマンド実行 (spawn)
local function spawn_p4(args, cwd, on_success)
    local stdout = vim.loop.new_pipe(false)
    local stderr = vim.loop.new_pipe(false)
    local output_data = ""

    local handle, pid
    handle, pid = vim.loop.spawn("p4", {
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
        on_success(nil)
    end
end

-- P4が利用可能かチェック (ディレクトリごとにキャッシュ)
local function check_availability(cwd, callback)
    local key = unl_path.normalize(cwd or "")
    if availability_cache[key] ~= nil then
        callback(availability_cache[key])
        return
    end
    spawn_p4({ "where", "." }, cwd, function(output)
        availability_cache[key] = (output ~= nil and output ~= "")
        callback(availability_cache[key])
    end)
end

function M.get_hash(root)
    -- submittedな最新のチェンジリストを取得
    local output = vim.fn.systemlist("p4 -d " .. vim.fn.shellescape(root) .. " changes -m1 -s submitted #have")
    if vim.v.shell_error == 0 and #output > 0 then
        local change = output[1]:match("Change (%d+)")
        if change then return "p4:" .. change end
    end
    return nil
end

--- プロジェクト全体のステータス更新
function M.refresh(start_path, on_complete, logger_name)
    if not start_path then return end
    
    check_availability(start_path, function(available)
        if not available then
            if on_complete then on_complete() end
            return
        end

        local args = { "-F", "%clientFile%|%action%", "opened", "..." }
        
        spawn_p4(args, start_path, function(output)
            local new_cache = {}
            if output then
                for line in output:gmatch("[^\r\n]+") do
                    local path_part, action = line:match("^(.*)|(.*)$")
                    if path_part and action then
                        local key = make_key(path_part)
                        
                        -- ステータスコードに変換
                        local status_code = "M" -- Default Modified
                        if action == "add" then status_code = "A"
                        elseif action == "delete" then status_code = "D"
                        elseif action == "move/add" then status_code = "R"
                        elseif action == "edit" then status_code = "M" 
                        end
                        
                        new_cache[key] = status_code
                    end
                end
            end
            
            p4_status_cache = new_cache
            if on_complete then on_complete() end
        end)
    end)
end

function M.get_status(path)
    if not path then return nil end
    return p4_status_cache[make_key(path)]
end

function M.clear()
    p4_status_cache = {}
end

-- ======================================================
-- 同期アクション (自動チェックアウト用)
-- ======================================================

function M.edit(path, logger_name)
    local log = require("UNL.logging").get(logger_name or "UNL")
    local key = make_key(path)
    local output = vim.fn.system("p4 edit " .. vim.fn.shellescape(path))
    
    if vim.v.shell_error == 0 then
        p4_status_cache[key] = "M"
        log.info("P4 Checked out: " .. vim.fn.fnamemodify(path, ":t"))
        return true
    else
        log.error("P4 Checkout Failed:\n" .. output)
        return false
    end
end

function M.revert(path, logger_name)
    local log = require("UNL.logging").get(logger_name or "UNL")
    local key = make_key(path)
    local output = vim.fn.system("p4 revert " .. vim.fn.shellescape(path))
    
    if vim.v.shell_error == 0 then
        p4_status_cache[key] = nil
        log.info("P4 Reverted: " .. vim.fn.fnamemodify(path, ":t"))
        return true
    else
        log.error("P4 Revert Failed:\n" .. output)
        return false
    end
end

function M.is_managed(path)
    if not path or path == "" then return false end
    local cmd = "p4 files -m1 " .. vim.fn.shellescape(path)
    local output = vim.fn.system(cmd)
    
    if vim.v.shell_error == 0 and output and output ~= "" then
        if output:match("no such file") or output:match("not on client") then
            return false
        end
        return true
    end
    return false
end

function M.get_changes()
    local changes = {}
    for path, status in pairs(p4_status_cache) do
        table.insert(changes, { path = path, status = status })
    end
    return changes
end

--- 2つのチェンジリスト間の変更ファイルリストを非同期で取得する
--- @param root string プロジェクトルートパス
--- @param old_hash string 前回のハッシュ（"p4:CL" 形式）
--- @param new_hash string 現在のハッシュ（"p4:CL" 形式）
--- @param callback function(files: string[]|nil) 変更ファイルのローカルパスリスト
function M.get_changed_files(root, old_hash, new_hash, callback)
    if not root or not old_hash or not new_hash then
        return callback(nil)
    end

    local old_cl = old_hash:gsub("^p4:", "")
    local new_cl = new_hash:gsub("^p4:", "")

    -- Step 1: depot上で変更されたファイルを取得
    -- "..." はcwd以下のdepotマッピングに限定
    spawn_p4({ "files", "...@>" .. old_cl .. ",@" .. new_cl }, root, function(output)
        if not output or output == "" then return callback({}) end

        -- "//depot/path/file.cpp#3 - edit change 12347 (text)" → depot path を抽出
        local depot_paths = {}
        local seen = {}
        for line in output:gmatch("[^\r\n]+") do
            local dp = line:match("^(//[^#]+)")
            if dp and not seen[dp] then
                seen[dp] = true
                table.insert(depot_paths, dp)
            end
        end

        if #depot_paths == 0 then return callback({}) end

        -- Step 2: depot path → local path に変換（-ztag で安全にパース）
        -- コマンドライン長制限を考慮してバッチ処理
        local all_local = {}
        local batch_size = 50
        local pending = math.ceil(#depot_paths / batch_size)

        for i = 1, #depot_paths, batch_size do
            local args = { "-ztag", "where" }
            for j = i, math.min(i + batch_size - 1, #depot_paths) do
                table.insert(args, depot_paths[j])
            end

            spawn_p4(args, root, function(where_output)
                if where_output then
                    -- -ztag 出力: "... path C:\Projects\path\file.cpp"
                    for line in where_output:gmatch("[^\r\n]+") do
                        local local_path = line:match("^%.%.%. path (.+)$")
                        if local_path then
                            table.insert(all_local, unl_path.normalize(local_path))
                        end
                    end
                end

                pending = pending - 1
                if pending == 0 then
                    callback(all_local)
                end
            end)
        end
    end)
end

return M
