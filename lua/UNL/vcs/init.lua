-- lua/UNL/vcs/init.lua
local M = {}
local unl_path = require("UNL.path")

local providers = {
    { name = "p4",  module = require("UNL.vcs.p4") },
    { name = "git", module = require("UNL.vcs.git") },
    { name = "svn", module = require("UNL.vcs.svn") },
}

---現在のプロジェクトのVCSハッシュ/リビジョンを取得する
-- @param root string プロジェクトのルートパス
-- @return string|nil ハッシュ文字列。VCS管理下でない場合はnil
function M.get_current_hash(root)
    if not root then return nil end
    root = unl_path.normalize(root)
    
    -- 優先順位: P4 -> Git -> SVN
    
    -- P4 Check
    local p4_hash = providers[1].module.get_hash(root)
    if p4_hash then return p4_hash end

    -- Git Check: .gitを上方向に探す
    local git_dir = vim.fn.finddir(".git", root .. ";")
    local git_file = vim.fn.findfile(".git", root .. ";")
    if git_dir ~= "" or git_file ~= "" then
        local git_hash = providers[2].module.get_hash(root)
        if git_hash then return git_hash end
    end

    -- SVN Check: .svnを上方向に探す
    local svn_dir = vim.fn.finddir(".svn", root .. ";")
    if svn_dir ~= "" then
        local svn_hash = providers[3].module.get_hash(root)
        if svn_hash then return svn_hash end
    end

    return nil
end

--- VCSの状態を非同期で更新する
function M.refresh(root, on_complete, logger_name)
    local pending = 0
    for i=1, 2 do -- P4, Git のみリフレッシュ対応 (SVNは現在未実装)
        if providers[i].module.refresh then pending = pending + 1 end
    end

    if pending == 0 then
        if on_complete then on_complete() end
        return
    end

    local function check_done()
        pending = pending - 1
        if pending <= 0 and on_complete then on_complete() end
    end

    for i=1, 2 do
        if providers[i].module.refresh then
            providers[i].module.refresh(root, check_done, logger_name)
        end
    end
end

--- 全てのVCS変更ファイルをマージして返す (Local Changes)
function M.get_aggregated_changes()
    local combined = {}
    local seen = {}

    for _, provider in ipairs(providers) do
        if type(provider.module.get_changes) == "function" then
            local changes = provider.module.get_changes()
            for _, item in ipairs(changes) do
                if not seen[item.path] then
                    seen[item.path] = true
                    table.insert(combined, item)
                end
            end
        end
    end
    table.sort(combined, function(a, b) return a.path < b.path end)
    return combined
end

--- 未プッシュのファイルをマージして返す (Git用)
function M.get_aggregated_unpushed()
    local combined = {}
    local seen = {}
    for _, provider in ipairs(providers) do
        if type(provider.module.get_unpushed) == "function" then
            local changes = provider.module.get_unpushed()
            for _, item in ipairs(changes) do
                if not seen[item.path] then
                    seen[item.path] = true
                    table.insert(combined, item)
                end
            end
        end
    end
    table.sort(combined, function(a, b) return a.path < b.path end)
    return combined
end

--- 2つのVCSリビジョン間の変更ファイルリストを非同期で取得する
--- @param root string プロジェクトルートパス
--- @param old_hash string 前回のハッシュ（"git:xxx" / "p4:xxx" / "svn:xxx"）
--- @param new_hash string 現在のハッシュ
--- @param callback function(files: string[]|nil) 変更ファイルの絶対パスリスト。未対応VCSの場合nil
function M.get_changed_files(root, old_hash, new_hash, callback)
    if not root or not old_hash or not new_hash then
        return callback(nil)
    end

    -- VCSプレフィックスで適切なプロバイダーを選択
    local prefix = old_hash:match("^(%a+):")
    if prefix == "git" and providers[2].module.get_changed_files then
        providers[2].module.get_changed_files(root, old_hash, new_hash, callback)
    elseif prefix == "p4" and providers[1].module.get_changed_files then
        providers[1].module.get_changed_files(root, old_hash, new_hash, callback)
    elseif prefix == "svn" and providers[3].module.get_changed_files then
        providers[3].module.get_changed_files(root, old_hash, new_hash, callback)
    else
        callback(nil)
    end
end

--- パスのVCSステータスを取得する
function M.get_status(path)
    for i=1, 2 do -- P4, Git
        local status = providers[i].module.get_status(path)
        if status then return status end
    end
    return nil
end

--- 全プロバイダーのキャッシュをクリア
function M.clear()
    for _, provider in ipairs(providers) do
        if type(provider.module.clear) == "function" then provider.module.clear() end
    end
end

-- P4 Helpers
function M.is_p4_managed(path)
    return providers[1].module.is_managed(path)
end

function M.p4_edit(path, logger_name)
    return providers[1].module.edit(path, logger_name)
end

function M.p4_revert(path, logger_name)
    return providers[1].module.revert(path, logger_name)
end

function M.get_file_content(path, on_success)
    local index = 1
    local function try_next()
        if index > #providers then return on_success(nil) end
        local provider = providers[index]
        index = index + 1
        if type(provider.module.get_file_content) == "function" then
            provider.module.get_file_content(path, function(content)
                if content then on_success(content) else try_next() end
            end)
        else
            try_next()
        end
    end
    try_next()
end

return M
