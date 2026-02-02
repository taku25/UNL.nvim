-- lua/UNL/vcs.lua
local M = {}
local unl_path = require("UNL.path")

---Gitの最新ハッシュを取得
local function get_git_hash(root)
    local output = vim.fn.systemlist("git -C " .. vim.fn.shellescape(root) .. " rev-parse HEAD")
    if vim.v.shell_error == 0 and #output > 0 then
        return "git:" .. output[1]
    end
    return nil
end

---Perforceの最新チェンジリストを取得
local function get_p4_hash(root)
    -- submittedな最新のチェンジリストを取得
    local output = vim.fn.systemlist("p4 -d " .. vim.fn.shellescape(root) .. " changes -m1 -s submitted #have")
    if vim.v.shell_error == 0 and #output > 0 then
        local change = output[1]:match("Change (%d+)")
        if change then return "p4:" .. change end
    end
    return nil
end

---SVNの最新リビジョンを取得
local function get_svn_hash(root)
    local output = vim.fn.systemlist("svn info --show-item revision " .. vim.fn.shellescape(root))
    if vim.v.shell_error == 0 and #output > 0 then
        return "svn:" .. output[1]
    end
    return nil
end

---現在のプロジェクトのVCSハッシュ/リビジョンを取得する
-- @param root string プロジェクトのルートパス
-- @return string|nil ハッシュ文字列。VCS管理下でない場合はnil
function M.get_current_hash(root)
    if not root then return nil end
    root = unl_path.normalize(root)
    
    -- 優先順位: P4 -> Git -> SVN
    
    -- P4 Check (p4 info は遅いのでディレクトリ存在チェックはできないが、まずは実行してみる)
    local p4 = get_p4_hash(root)
    if p4 then return p4 end

    -- Git Check
    if vim.fn.isdirectory(root .. "/.git") == 1 or vim.fn.filereadable(root .. "/.git") == 1 then
        local git = get_git_hash(root)
        if git then return git end
    end

    -- SVN Check
    if vim.fn.isdirectory(root .. "/.svn") == 1 then
        local svn = get_svn_hash(root)
        if svn then return svn end
    end

    return nil
end

return M
