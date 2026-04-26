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

--- 現在の SVN ユーザー名を取得する
--- @param cwd string
--- @param callback function(name: string|nil)
function M.get_user_name(cwd, callback)
    spawn_svn({ "info", "--show-item", "wc-root" }, cwd, function(output)
        if not output then return callback(nil) end
        spawn_svn({ "log", "-l", "1", "--xml" }, cwd, function(log_output)
            if not log_output then return callback(nil) end
            local author = log_output:match("<author>([^<]+)</author>")
            callback(author)
        end)
    end)
end

--- SVN ログを取得する
--- @param cwd string
--- @param limit number 最大取得件数
--- @param author string|nil 著者フィルタ
--- @param callback function(commits: table[]|nil)
function M.get_log(cwd, limit, author, callback)
    spawn_svn({ "info", "--show-item", "wc-root" }, cwd, function(wc_output)
        if not wc_output then return callback(nil) end
        local fetch_limit = author and tostring(limit * 5) or tostring(limit)
        spawn_svn({ "log", "-l", fetch_limit, "--xml" }, cwd, function(output)
            if not output then return callback(nil) end
            local commits = {}
            for entry in output:gmatch("<logentry(.-)</logentry>") do
                local rev          = entry:match('revision="(%d+)"')
                local entry_author = entry:match("<author>([^<]*)</author>") or ""
                local date_str     = entry:match("<date>([^<]*)</date>") or ""
                local msg          = entry:match("<msg>([^<]*)</msg>") or "(no message)"
                if rev then
                    local include = not (author and entry_author ~= author)
                    if include and #commits < limit then
                        local rel_date = date_str
                        local y, mo, d, h, mi, s = date_str:match("(%d+)-(%d+)-(%d+)T(%d+):(%d+):(%d+)")
                        if y then
                            local ts = os.time({ year=y, month=mo, day=d, hour=h, min=mi, sec=s })
                            local diff = os.time() - ts
                            if diff < 3600 then
                                rel_date = math.floor(diff/60) .. " minutes ago"
                            elseif diff < 86400 then
                                rel_date = math.floor(diff/3600) .. " hours ago"
                            else
                                rel_date = math.floor(diff/86400) .. " days ago"
                            end
                        end
                        table.insert(commits, {
                            hash    = "r" .. rev,
                            message = vim.fn.trim(msg),
                            author  = entry_author,
                            date    = rel_date,
                            display = string.format("r%s %s (%s)", rev, vim.fn.trim(msg), rel_date),
                            vcs     = "svn",
                            _rev    = rev,
                        })
                    end
                end
            end
            callback(commits)
        end)
    end)
end

--- SVN リビジョンの変更ファイル一覧を取得する
--- @param cwd string
--- @param revision string リビジョン番号 ("r123" 形式も可)
--- @param callback function(items: table[]|nil)
function M.get_commit_files(cwd, revision, callback)
    local rev = revision:gsub("^r", "")
    spawn_svn({ "log", "-r", rev, "-v", "--xml" }, cwd, function(output)
        if not output then return callback(nil) end
        local files = {}
        for path_entry in output:gmatch("<path(.-)</path>") do
            local path = path_entry:match(">(.+)$")
            if path then
                table.insert(files, {
                    type          = "file",
                    path          = path,
                    name          = vim.fn.fnamemodify(path, ":t"),
                    full_rel_path = path,
                })
            end
        end
        callback(files)
    end)
end

return M
