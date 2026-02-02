local scanner = require("UNL.scanner")
local path_util = require("UNL.path")
local finder = require("UNL.finder")
local log = require("UNL.logging").get("UNL")

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
        local db_path = path_util.get_db_path(project_root)
        local current_vcs = vcs.get_current_hash(project_root)
        
        local config = {
            include_extensions = {"uproject", "cpp", "h", "hpp", "inl", "ini", "cs"},
            excludes_directory = {"Intermediate", "Binaries", "Saved", ".git", ".vs", "Templates"},
        }

        local req = {
            project_root = path_util.normalize(project_root),
            db_path = path_util.normalize(db_path),
            config = config,
            vcs_hash = current_vcs,
        }

        log.debug("Setting up UNL for project: %s (VCS: %s)", project_root, current_vcs or "None")
        
        rpc.request("setup", req, nil, function(success, result_or_err)
            if success then
                log.debug("UNL setup complete.")
            else
                log.error("UNL setup failed: %s", result_or_err or "Unknown error")
            end
            if on_complete then on_complete(success) end
        end)
    end)
end

return M