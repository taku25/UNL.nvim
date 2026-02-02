-- lua/UNL/cmd/refresh.lua (Server-Aware Progress Reporting)
local scanner = require("UNL.scanner")
local path_util = require("UNL.path")
local finder = require("UNL.finder")
local log = require("UNL.logging").get("UNL")
local unl_config = require("UNL.config")
local progress_backend = require("UNL.backend.progress")
local unl_events = require("UNL.event.events")
local unl_event_types = require("UNL.event.types")

local M = {}

local rpc = require("UNL.rpc")
local server_manager = require("UNL.scanner.server")
local vcs = require("UNL.vcs")

function M.execute(opts, on_complete)
    opts = opts or {}
    
    server_manager.ensure_running(function(ok)
        if not ok then
            if on_complete then on_complete(false) end
            return
        end

        local project_info = finder.project.find_project(vim.loop.cwd())
        if not (project_info and project_info.uproject) then
            log.error("Could not find a .uproject file.")
            if on_complete then on_complete(false) end
            return
        end
        
        local project_root = vim.fn.fnamemodify(project_info.uproject, ":h")
        local engine_root = finder.engine.find_engine_root(project_info.uproject, {})
        local current_vcs = vcs.get_current_hash(project_root)
        
        local config = {
            include_extensions = {"uproject", "cpp", "h", "hpp", "inl", "ini", "cs", "usf", "ush"},
            excludes_directory = {"Intermediate", "Binaries", "Saved", ".git", ".vs", "Templates"},
        }

        local req = {
            type = "refresh",
            project_root = path_util.normalize(project_root),
            engine_root = engine_root and path_util.normalize(engine_root) or nil,
            db_path = path_util.get_db_path(project_root), 
            scope = opts.scope or "Full",
            config = config,
            vcs_hash = current_vcs,
        }

        -- Initialize Progress Backend
        local progress, _ = progress_backend.create_for_refresh(unl_config.get("UNL"), {
            title = "UNL Refresh: " .. vim.fn.fnamemodify(project_root, ":t"),
            client_name = "UNL.Server",
            weights = {
                discovery = 0.05,
                db_sync = 0.2,
                file_scan = 0.05,
                analysis = 0.6,
                finalizing = 0.1 -- 10% weight for finalization
            }
        })
        progress:open()
        
        -- Ensure stages are defined
        progress:stage_define("discovery", 100)
        progress:stage_define("db_sync", 100)
        progress:stage_define("file_scan", 10000) -- Auto-grows
        progress:stage_define("analysis", 1000)   -- Auto-grows
        progress:stage_define("finalizing", 100)

        log.debug("Requesting refresh for: %s (Scope: %s)", project_root, req.scope)
        
        rpc.request("refresh", req, function(method, msg)
            local stage = msg.stage or msg[2]
            local current = msg.current or msg[3]
            local message = msg.message or msg[5]
            
            if method == "progress" and stage then
                progress:stage_update(stage, current, message)
            end
        end, function(success, result_or_err)
            progress:finish(success)
            if success then
                log.debug("Refresh completed successfully.")
                unl_events.publish(unl_event_types.ON_AFTER_REFRESH_COMPLETED, { project_root = project_root })
            else
                local err_msg = result_or_err or "Unknown error"
                log.error("Refresh failed: %s", err_msg)
            end
            if on_complete then on_complete(success) end
        end, 600000) -- 10 minutes timeout
    end)
end

return M