local path_util = require("UNL.path")
local finder = require("UNL.finder")
local log = require("UNL.logging").get("UNL")
local rpc = require("UNL.rpc")
local server_manager = require("UNL.scanner.server")

local M = {}

function M.execute(opts, on_complete)
    opts = opts or {}

    server_manager.ensure_running(function(ok)
        if not ok then
            if on_complete then on_complete(false) end
            return
        end

        local cwd = vim.loop.cwd()
        local project_info = finder.project.find_project(cwd)
        if not (project_info and project_info.uproject) then
            log.error("Could not find a .uproject file.")
            if on_complete then on_complete(false) end
            return
        end

        local project_root = vim.fn.fnamemodify(project_info.uproject, ":h")
        local project_root_norm = path_util.normalize(project_root)

        log.info("Starting manual asset rescan for: %s", vim.fn.fnamemodify(project_root, ":t"))

        rpc.request("rescan_assets", { project_root = project_root_norm }, nil, function(success, result_or_err)
            if success then
                local status = type(result_or_err) == "table" and result_or_err.status or "ok"
                if status == "already_scanning" then
                    log.warn("Asset scan is already in progress.")
                else
                    log.info("Asset rescan started. (Background)")
                end
                if on_complete then on_complete(true) end
            else
                log.error("Asset rescan failed: %s", tostring(result_or_err))
                if on_complete then on_complete(false) end
            end
        end)
    end)
end

return M
