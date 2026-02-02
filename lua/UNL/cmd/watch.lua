local scanner = require("UNL.scanner")
local path_util = require("UNL.path")
local finder = require("UNL.finder")
local log = require("UNL.logging").get("UNL")

local M = {}

local rpc = require("UNL.rpc")
local server_manager = require("UNL.scanner.server")

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

        local req = {
            project_root = path_util.normalize(project_root),
            db_path = nil, -- Implicit
        }

        log.debug("Requesting watcher start for: %s", project_root)
        
        rpc.request("watch", req, nil, function(success, result_or_err)
            if success then
                log.debug("Watcher confirmed by server.")
            else
                log.error("Failed to start watcher on server: %s", result_or_err or "Unknown error")
            end
            if on_complete then on_complete(success) end
        end)
    end)
end

return M