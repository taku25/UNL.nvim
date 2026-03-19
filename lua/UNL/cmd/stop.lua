local server = require("UNL.scanner.server")
local vcs_poller = require("UNL.vcs.poller")
local M = {}

function M.execute()
    vcs_poller.stop_all()
    server.stop()
    local log = require("UNL.logging").get("UNL")
    log.info("Server stopped.")
end

return M