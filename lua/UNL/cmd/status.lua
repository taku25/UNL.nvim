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

        -- 2. Fetch detailed status via RPC (accumulate lines, parse once complete)
        local lines = {}
        scanner.run_command("status", {}, function(line)
            table.insert(lines, line)
        end, function(_ok)
            local raw = table.concat(lines, "\n")
            local parse_ok, msg = pcall(vim.json.decode, raw)
            if parse_ok and type(msg) == "table" then
                print("--- UNL Server Status ---")
                print("Status: " .. tostring(msg.status))
                print("Active Projects:")
                for _, p in ipairs(msg.active_projects or {}) do
                    print("  - " .. p)
                end
            else
                print("--- UNL Server Status ---")
                print(raw ~= "" and raw or "(no response)")
            end
        end)
    end)
end

return M