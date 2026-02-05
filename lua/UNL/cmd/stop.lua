local server = require("UNL.scanner.server")
local M = {}

function M.execute()
    server.stop()
    local log = require("UNL.logging").get("UNL")
    log.info("Server stopped.")
end

return M