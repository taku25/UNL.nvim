local server = require("UNL.scanner.server")
local M = {}

function M.execute()
    local log = require("UNL.logging").get("UNL")
    server.stop()
    log.info("Restarting server...")
    vim.defer_fn(function()
        server.start(function(ok)
            if ok then
                log.info("Server restarted successfully.")
            else
                log.error("Failed to restart server.")
            end
        end)
    end, 500)
end

return M