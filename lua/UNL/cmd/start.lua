-- lua/UNL/cmd/start.lua (Robust Server Startup)
local setup = require("UNL.cmd.setup")
local watch = require("UNL.cmd.watch")
local refresh = require("UNL.cmd.refresh")
local server_manager = require("UNL.scanner.server")
local log = require("UNL.logging").get("UNL")
local rpc = require("UNL.rpc")
local vcs = require("UNL.vcs")
local path_util = require("UNL.path")
local finder = require("UNL.finder")

local M = {}

local active_starts = {}

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
                    
                    local db_exists = false
                    if vim.fn.filereadable(db_path) == 1 then
                        local size = vim.fn.getfsize(db_path)
                        if size > 50000 then -- Over 50KB considered valid
                            db_exists = true
                        end
                    end
                    
                    local current_vcs = vcs.get_current_hash(project_root)

                    if is_registered and db_exists then
                        local vcs_changed = (current_vcs ~= last_vcs) and (current_vcs ~= nil)
                        if vcs_changed then
                             log.info("VCS changed for %s. Refreshing...", project_root)
                             refresh.execute(opts)
                        else
                             log.debug("UNL Server is ready for project: %s", project_root)
                        end
                        -- 一旦完了とみなす (vcs_changed の場合でも refresh 内で管理される)
                        active_starts[project_root_norm] = nil
                        return
                    end

                    if is_registered and not db_exists then
                        log.info("Database missing for %s. Starting full refresh...", project_root)
                        local refresh_opts = vim.tbl_extend("force", opts, { scope = "Full" })
                        refresh.execute(refresh_opts)
                        watch.execute(opts)
                        active_starts[project_root_norm] = nil
                        return
                    end

                    log.info("Registering and initializing project: %s", project_root)
                    setup.execute(opts, function(setup_success)
                        if setup_success then
                            watch.execute(opts)
                            local refresh_opts = vim.tbl_extend("force", opts, { scope = "Full" })
                            refresh.execute(refresh_opts)
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