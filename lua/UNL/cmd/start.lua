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

function M.execute(opts)
    opts = opts or {}
    
    -- 1. Start Server Process (Idempotent)
    server_manager.start()
    
    -- 2. Wait for Server to bind TCP port
    local retries = 10
    local function poll_and_setup()
        server_manager.get_status(function(status)
            if status and status.status == "running" then
                -- log.debug("UNL Server is up on port %d.", status.port)
                
                local project_info = finder.project.find_project(vim.loop.cwd())
                if not (project_info and project_info.uproject) then
                    log.error("Could not find a .uproject file.")
                    return
                end
                local project_root = vim.fn.fnamemodify(project_info.uproject, ":h")
                local project_root_norm = path_util.normalize(project_root)
                local db_path = path_util.get_db_path(project_root)
                local current_vcs = vcs.get_current_hash(project_root)

                -- 3. Check if project is already registered
                rpc.request("list_projects", {}, nil, function(ok, projects)
                    local is_registered = false
                    local last_vcs = nil
                    
                    if ok and type(projects) == "table" then
                        for _, p in ipairs(projects) do
                            if path_util.equal(p.root, project_root_norm) then
                                is_registered = true
                                last_vcs = p.vcs_hash
                                break
                            end
                        end
                    end
                    
                    local db_exists = vim.fn.filereadable(db_path) == 1
                    
                    if is_registered and db_exists then
                        -- Already running and setup. Just check for updates quietly.
                        local vcs_changed = (current_vcs ~= last_vcs) and (current_vcs ~= nil)
                        
                        if vcs_changed then
                             log.debug("VCS state changed (%s -> %s). Refreshing...", last_vcs or "None", current_vcs)
                             refresh.execute(opts)
                        else
                             log.debug("UNL Server is running.")
                        end
                        return
                    end

                    log.debug("Setting up UNL for project: %s", project_root)

                    -- 4. Perform initial setup
                    setup.execute(opts, function(success)
                        if success then
                            -- 5. Start watcher
                            watch.execute(opts)
                            
                            -- 6. Smart Refresh Check
                            local db_size = 0
                            if db_exists then
                                db_size = vim.fn.getfsize(db_path)
                            end

                            local vcs_changed = (current_vcs ~= last_vcs) and (current_vcs ~= nil)
                            local db_is_empty = db_exists and (db_size < 500000) -- 500KB check

                            if not db_exists or db_is_empty then
                                if db_is_empty then
                                    log.debug("Database appears incomplete. Triggering Full refresh...")
                                else
                                    log.debug("Database not found. Triggering initial Full refresh...")
                                end
                                local refresh_opts = vim.tbl_extend("force", opts, { scope = "Full" })
                                refresh.execute(refresh_opts)
                            elseif vcs_changed then
                                log.debug("VCS state changed. Refreshing...")
                                refresh.execute(opts)
                            else
                                log.debug("UNL is up-to-date.")
                            end
                        end
                    end)
                end)
            elseif retries > 0 then
                retries = retries - 1
                vim.defer_fn(poll_and_setup, 500)
            else
                log.error("UNL Server failed to start or bind to port 30010.")
            end
        end)
    end
    
    log.debug("Starting UNL services...")
    vim.defer_fn(poll_and_setup, 500)
end

return M