-- lua/UNL/cmd/start.lua (Robust Server Startup)
local setup = require("UNL.cmd.setup")
local watch = require("UNL.cmd.watch")
local refresh = require("UNL.cmd.refresh")
local server_manager = require("UNL.scanner.server")
local log = require("UNL.logging").get("UNL")
local rpc = require("UNL.rpc")
local vcs = require("UNL.vcs")
local vcs_poller = require("UNL.vcs.poller")
local path_util = require("UNL.path")
local finder = require("UNL.finder")

local M = {}

local active_starts = {}
local completed_setups = {}

function M.execute(opts)
    opts = opts or {}
    
    -- プロジェクトルートを取得して、既に開始処理中ならスキップ
    local cwd = vim.loop.cwd()
    local project_info = finder.project.find_project(cwd)
    if not (project_info and project_info.uproject) then
        local buf_path = vim.api.nvim_buf_get_name(0)
        if buf_path ~= "" then
            project_info = finder.project.find_project(vim.fn.fnamemodify(buf_path, ":p:h"))
        end
    end

    if project_info and project_info.uproject then
        local root_norm = path_util.normalize(vim.fn.fnamemodify(project_info.uproject, ":h"))
        if active_starts[root_norm] then
            log.debug("Start already in progress for %s. Skipping.", root_norm)
            return
        end
        -- セッション内で既にセットアップ済みかつ強制フラグなし
        -- ただし、サーバーが実際に動いているか確認してからスキップ
        if completed_setups[root_norm] and not opts.force then
            server_manager.get_status(function(status)
                if not status then
                    -- サーバーが停止していたため、キャッシュをクリアして再登録する
                    log.debug("Server is down for %s despite prior setup. Clearing cache and re-registering...", root_norm)
                    completed_setups[root_norm] = nil
                    M.execute(opts)
                else
                    local current_vcs = vcs.get_current_hash(root_norm)
                    vcs_poller.start(root_norm, current_vcs)
                end
            end)
            return
        end
        active_starts[root_norm] = true
    end

    server_manager.start()
    
    local retries = 30
    local function poll_and_setup()
        rpc.request("ping", { pid = vim.loop.os_getpid() }, nil, function(ok, _)
            if ok then
                -- Try to find project again (in case it wasn't found before)
                if not (project_info and project_info.uproject) then
                    project_info = finder.project.find_project(vim.loop.cwd())
                    if not (project_info and project_info.uproject) then
                        local buf_path = vim.api.nvim_buf_get_name(0)
                        if buf_path ~= "" then
                            project_info = finder.project.find_project(vim.fn.fnamemodify(buf_path, ":p:h"))
                        end
                    end
                end

                if not (project_info and project_info.uproject) then
                    if retries > 0 then
                        retries = retries - 1
                        vim.defer_fn(poll_and_setup, 1000)
                    end
                    return
                end
                
                local project_root = vim.fn.fnamemodify(project_info.uproject, ":h")
                local project_root_norm = path_util.normalize(project_root)
                local db_path = path_util.get_db_path(project_root)
                
                rpc.request("list_projects", {}, nil, function(list_ok, projects)
                    if not list_ok then 
                        active_starts[project_root_norm] = nil
                        return 
                    end

                    local is_registered = false
                    local last_vcs = nil
                    
                    if type(projects) == "table" then
                        for _, p in ipairs(projects) do
                            if path_util.equal(p.root, project_root_norm) then
                                is_registered = true
                                last_vcs = p.vcs_hash
                                break
                            end
                        end
                    end
                    
                    local current_vcs = vcs.get_current_hash(project_root)
                    local vcs_changed = is_registered and (current_vcs ~= last_vcs) and (current_vcs ~= nil)

                    -- セッション内ですでにセットアップ済み、かつVCS変更なしならスキップ
                    if completed_setups[project_root_norm] and not vcs_changed and not opts.force then
                        log.debug("Project %s already setup in this session. Skipping redundant registration.", project_root)
                        active_starts[project_root_norm] = nil
                        -- ポーラーだけ確実に動いているか確認
                        vcs_poller.start(project_root_norm, current_vcs)
                        return
                    end

                    log.info("Registering and initializing project: %s", project_root)
                    setup.execute(opts, function(setup_res)
                        log.debug("Setup callback received. Result: %s", vim.inspect(setup_res))
                        
                        local is_ok = false
                        local needs_full_refresh = false
                        
                        if type(setup_res) == "table" then
                            is_ok = (setup_res.status == "ok")
                            needs_full_refresh = setup_res.needs_full_refresh
                        elseif setup_res == true then
                            is_ok = true
                        end

                        if is_ok then
                            log.debug("Setup confirmed OK. Triggering watcher...")
                            completed_setups[project_root_norm] = true
                            
                            -- 明示的にルートを渡す
                            local watch_opts = vim.tbl_extend("force", opts or {}, { project_root = project_root_norm })
                            watch.execute(watch_opts, function(watch_ok)
                                log.debug("Watcher execution result: %s", tostring(watch_ok))
                            end)
                            
                            if needs_full_refresh or vcs_changed then
                                if vcs_changed then
                                    log.info("VCS changed for %s. Refreshing...", project_root)
                                else
                                    log.info("Database empty or re-initialized. Starting full refresh...")
                                end
                                local refresh_opts = vim.tbl_extend("force", opts, { scope = "Full" })
                                refresh.execute(refresh_opts, function()
                                    -- Full refresh 完了後にポーラーを起動（最新ハッシュで）
                                    vcs_poller.start(project_root_norm, current_vcs)
                                end)
                            else
                                log.debug("UNL Server is ready for project: %s", project_root)
                                -- リフレッシュ不要 → 既存ハッシュでポーラー起動
                                vcs_poller.start(project_root_norm, current_vcs)
                            end
                        else
                            log.error("Setup failed or returned invalid response: %s", vim.inspect(setup_res))
                        end
                        active_starts[project_root_norm] = nil
                    end)
                end)
            elseif retries > 0 then
                retries = retries - 1
                vim.defer_fn(poll_and_setup, 500)
            else
                -- Timeout
                if project_info and project_info.uproject then
                    local root_norm = path_util.normalize(vim.fn.fnamemodify(project_info.uproject, ":h"))
                    active_starts[root_norm] = nil
                end
            end
        end)
    end
    
    vim.defer_fn(poll_and_setup, 200)
end

return M