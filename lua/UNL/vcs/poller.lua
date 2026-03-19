-- lua/UNL/vcs/poller.lua
-- VCS変更検知 → インクリメンタル or フルリフレッシュの自動トリガー
local vcs = require("UNL.vcs")
local rpc = require("UNL.rpc")
local path_util = require("UNL.path")
local log = require("UNL.logging").get("UNL")
local unl_config = require("UNL.config")
local unl_events = require("UNL.event.events")
local unl_event_types = require("UNL.event.types")
local refresh_cmd = require("UNL.cmd.refresh")

local M = {}

-- { [project_root_norm] = { vcs_hash, last_check, augroup_id } }
local watched_projects = {}
local augroup = nil

--- 変更ファイルリストに構造変更が含まれるかチェック
--- @param files string[]
--- @param patterns string[]
--- @return boolean
local function has_structural_changes(files, patterns)
    for _, file in ipairs(files) do
        for _, pat in ipairs(patterns) do
            if file:match(pat) then
                return true
            end
        end
    end
    return false
end

--- 対象拡張子のファイルだけをフィルタリング
--- @param files string[]
--- @return string[]
local function filter_scannable_files(files)
    local exts = { h = true, hpp = true, cpp = true, cs = true, inl = true }
    local result = {}
    for _, file in ipairs(files) do
        local ext = file:match("%.(%w+)$")
        if ext and exts[ext:lower()] then
            table.insert(result, file)
        end
    end
    return result
end

--- 変更ファイルをサーバーの scan リクエストで送信（インクリメンタル更新）
--- @param project_root string
--- @param files string[]
local function incremental_scan(project_root, files)
    local db_path = path_util.get_db_path(project_root)
    local scan_files = {}
    for _, file_path in ipairs(files) do
        local mtime = 0
        local stat = vim.loop.fs_stat(file_path)
        if stat then
            mtime = stat.mtime and stat.mtime.sec or 0
        end
        table.insert(scan_files, {
            path = path_util.normalize(file_path),
            mtime = mtime,
            old_hash = nil, -- 強制リパース
            module_id = nil, -- サーバー側で解決
            db_path = db_path,
        })
    end

    rpc.request("scan", { files = scan_files }, nil, function(ok, result)
        if ok then
            log.info("VCS auto-refresh: %d files incrementally updated.", #scan_files)
            unl_events.publish(unl_event_types.ON_AFTER_VCS_AUTO_REFRESH, {
                project_root = project_root,
                mode = "incremental",
                file_count = #scan_files,
            })
        else
            log.error("VCS auto-refresh (incremental) failed: %s", tostring(result))
        end
    end)
end

--- VCSハッシュの変化を検知して適切なリフレッシュを実行
--- @param root_norm string
local function check_and_refresh(root_norm)
    local state = watched_projects[root_norm]
    if not state then return end

    local conf = unl_config.get("UNL")
    local auto_conf = conf and conf.vcs and conf.vcs.auto_refresh
    if not auto_conf or not auto_conf.enabled then return end

    -- クールダウンチェック
    local now = os.time()
    local cooldown = auto_conf.cooldown or 300
    if (now - state.last_check) < cooldown then return end
    state.last_check = now

    local current_hash = vcs.get_current_hash(root_norm)
    if not current_hash then return end
    if current_hash == state.vcs_hash then return end

    local old_hash = state.vcs_hash
    state.vcs_hash = current_hash

    log.info("VCS change detected: %s → %s", tostring(old_hash), current_hash)

    -- 変更ファイルリストを取得して分岐
    vcs.get_changed_files(root_norm, old_hash, current_hash, function(changed_files)
        if not changed_files then
            -- VCSが差分取得未対応（P4/SVN）→ Full Refresh
            log.info("VCS auto-refresh: diff unavailable, running full refresh.")
            refresh_cmd.execute({ scope = "Full" }, function(ok)
                unl_events.publish(unl_event_types.ON_AFTER_VCS_AUTO_REFRESH, {
                    project_root = root_norm,
                    mode = "full",
                    reason = "diff_unavailable",
                })
            end)
            return
        end

        if #changed_files == 0 then
            log.debug("VCS hash changed but no file diffs (merge commit, etc). Skipping.")
            return
        end

        local threshold = auto_conf.full_refresh_threshold or 100
        local patterns = auto_conf.structural_patterns or {}

        -- ケースA: 構造ファイルが変わった → Full Refresh
        if has_structural_changes(changed_files, patterns) then
            log.info("VCS auto-refresh: structural change detected (%d files). Running full refresh.", #changed_files)
            refresh_cmd.execute({ scope = "Full" }, function(ok)
                unl_events.publish(unl_event_types.ON_AFTER_VCS_AUTO_REFRESH, {
                    project_root = root_norm,
                    mode = "full",
                    reason = "structural_change",
                })
            end)
            return
        end

        -- スキャン対象の拡張子のみフィルタ
        local scannable = filter_scannable_files(changed_files)

        -- ケースC: 大量変更 → Full Refresh
        if #scannable > threshold then
            log.info("VCS auto-refresh: %d files changed (threshold: %d). Running full refresh.", #scannable, threshold)
            refresh_cmd.execute({ scope = "Full" }, function(ok)
                unl_events.publish(unl_event_types.ON_AFTER_VCS_AUTO_REFRESH, {
                    project_root = root_norm,
                    mode = "full",
                    reason = "threshold_exceeded",
                    file_count = #scannable,
                })
            end)
            return
        end

        -- ケースB: 少数のソースファイルのみ → Incremental Scan
        if #scannable > 0 then
            log.info("VCS auto-refresh: %d files changed. Running incremental scan.", #scannable)
            incremental_scan(root_norm, scannable)
        else
            log.debug("VCS auto-refresh: %d files changed but none are scannable. Skipping.", #changed_files)
        end
    end)
end

--- ポーラーを開始する
--- @param project_root string プロジェクトルート（正規化済み）
--- @param initial_vcs_hash string|nil 起動時の VCS ハッシュ
function M.start(project_root, initial_vcs_hash)
    local root_norm = path_util.normalize(project_root)
    local conf = unl_config.get("UNL")
    local auto_conf = conf and conf.vcs and conf.vcs.auto_refresh
    if not auto_conf or not auto_conf.enabled then
        log.debug("VCS auto-refresh is disabled.")
        return
    end

    -- 既に監視中なら更新のみ
    if watched_projects[root_norm] then
        watched_projects[root_norm].vcs_hash = initial_vcs_hash
        return
    end

    watched_projects[root_norm] = {
        vcs_hash = initial_vcs_hash,
        last_check = os.time(),
    }

    -- FocusGained autocmd（全プロジェクト共有で1回だけ作成）
    if not augroup and auto_conf.on_focus then
        augroup = vim.api.nvim_create_augroup("UNL_VcsAutoRefresh", { clear = true })
        vim.api.nvim_create_autocmd("FocusGained", {
            group = augroup,
            callback = function()
                for root, _ in pairs(watched_projects) do
                    check_and_refresh(root)
                end
            end,
        })
        log.debug("VCS auto-refresh: FocusGained watcher registered.")
    end
end

--- 指定プロジェクトのポーラーを停止
--- @param project_root string
function M.stop(project_root)
    local root_norm = path_util.normalize(project_root)
    watched_projects[root_norm] = nil
end

--- 全ポーラーを停止
function M.stop_all()
    watched_projects = {}
    if augroup then
        vim.api.nvim_del_augroup_by_id(augroup)
        augroup = nil
    end
end

return M
