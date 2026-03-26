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

        local project_root = opts.project_root
        if not project_root then
            local project_info = finder.project.find_project(vim.loop.cwd())
            if not (project_info and project_info.uproject) then
                -- Try to find based on current buffer if CWD failed
                local buf_path = vim.api.nvim_buf_get_name(0)
                if buf_path ~= "" then
                    project_info = finder.project.find_project(vim.fn.fnamemodify(buf_path, ":p:h"))
                end
            end

            if project_info and project_info.uproject then
                project_root = vim.fn.fnamemodify(project_info.uproject, ":h")
            end
        end

        if not project_root then
            log.error("Watcher: Could not identify project root.")
            if on_complete then on_complete(false) end
            return
        end
        
        local project_root_norm = path_util.normalize(project_root)

        local req = {
            project_root = project_root_norm,
            db_path = nil, -- Implicit
        }

        log.debug("RPC: Requesting watcher/watch for: %s", project_root_norm)
        
        rpc.request("watch", req, nil, function(success, result_or_err)
            if success then
                log.debug("RPC: Watcher confirmed by server for: %s", project_root_norm)
            else
                log.error("RPC: Failed to start watcher on server: %s", result_or_err or "Unknown error")
            end
            if on_complete then on_complete(success) end
        end)
    end)
end

return M