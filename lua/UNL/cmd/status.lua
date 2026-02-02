local scanner = require("UNL.scanner")
local log = require("UNL.logging").get("UNL")
local server_manager = require("UNL.scanner.server")

local M = {}

function M.execute(opts)
    -- 1. Check TCP Status
    server_manager.get_status(function(status)
        if not status then
            print("--- UNL Server Status ---")
            print("Status: NOT RUNNING (TCP Port 30010 is closed)")
            print("Try running :UNL start")
            return
        end

        -- 2. Fetch detailed status via RPC
        scanner.run_command("status", {}, function(line)
            local ok, msg = pcall(vim.json.decode, line)
            if ok then
                print("--- UNL Server Status ---")
                print("Status: " .. tostring(msg.status))
                print("Active Projects:")
                for _, p in ipairs(msg.active_projects or {}) do
                    print("  - " .. p)
                end
            else
                print("Raw Status: " .. line)
            end
        end)
    end)
end

return M