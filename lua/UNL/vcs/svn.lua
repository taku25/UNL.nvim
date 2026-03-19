-- lua/UNL/vcs/svn.lua
local unl_path = require("UNL.path")
local M = {}

-- 非同期 SVN コマンド実行
local function spawn_svn(args, cwd, on_success)
    local stdout = vim.loop.new_pipe(false)
    local stderr = vim.loop.new_pipe(false)
    local output_data = ""

    local handle, pid
    handle, pid = vim.loop.spawn("svn", {
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

function M.get_hash(root)
    local output = vim.fn.systemlist("svn info --show-item revision " .. vim.fn.shellescape(root))
    if vim.v.shell_error == 0 and #output > 0 then
        return "svn:" .. output[1]
    end
    return nil
end

--- 2つのリビジョン間の変更ファイルリストを非同期で取得する
--- @param root string プロジェクトルートパス
--- @param old_hash string 前回のハッシュ（"svn:REV" 形式）
--- @param new_hash string 現在のハッシュ（"svn:REV" 形式）
--- @param callback function(files: string[]|nil) 変更ファイルの絶対パスリスト
function M.get_changed_files(root, old_hash, new_hash, callback)
    if not root or not old_hash or not new_hash then
        return callback(nil)
    end

    local old_rev = old_hash:gsub("^svn:", "")
    local new_rev = new_hash:gsub("^svn:", "")

    -- svn diff --summarize -r OLD:NEW で変更ファイルを取得
    -- 出力: "M       /full/path/to/file.cpp" (ステータス + パス)
    spawn_svn({ "diff", "--summarize", "-r", old_rev .. ":" .. new_rev, root }, root, function(output)
        if not output then return callback(nil) end

        local files = {}
        for line in output:gmatch("[^\r\n]+") do
            -- ステータス文字（1-7カラム）の後にパス
            local path = line:match("^%S+%s+(.+)$")
            if path then
                local status_char = line:sub(1, 1)
                -- 削除されたファイルはスキップ（DBに残す意味がない）
                if status_char ~= "D" then
                    table.insert(files, unl_path.normalize(vim.fn.trim(path)))
                end
            end
        end
        callback(files)
    end)
end

return M
