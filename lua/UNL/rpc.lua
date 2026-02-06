local uv = vim.loop
local log = require("UNL.logging").get("UNL")

local M = {}

local msgid_counter = 0

function M.request(method, params, on_notification, on_response, timeout_ms)
    local ok_conf, config = pcall(require, "UNL.config")
    local conf = ok_conf and config.get("UNL")
    local port = (conf and conf.remote and conf.remote.port) or 30110
    local host = (conf and conf.remote and conf.remote.host) or "127.0.0.1"
    
    timeout_ms = timeout_ms or 30000

    msgid_counter = msgid_counter + 1
    local msgid = msgid_counter
    
    -- Structure: [type(0=request), msgid, method, params]
    local request_body = { 0, msgid, method, params }
    local encoded = vim.mpack.encode(request_body)
    
    -- 4 bytes length prefix (Big-Endian)
    local len = #encoded
    local header = string.char(
        bit.band(bit.rshift(len, 24), 0xFF),
        bit.band(bit.rshift(len, 16), 0xFF),
        bit.band(bit.rshift(len, 8), 0xFF),
        bit.band(len, 0xFF)
    )
    
    local client = uv.new_tcp()
    
    local timeout_timer = uv.new_timer()
    timeout_timer:start(timeout_ms, 0, function()
        if client then
            client:close()
            if on_response then
                vim.schedule(function() on_response(false, "RPC Timeout after " .. (timeout_ms/1000) .. "s") end)
            end
        end
    end)

    client:connect(host, port, function(err)
        if err then
            timeout_timer:stop()
            timeout_timer:close()
            if client then client:close() end
            if on_response then 
                vim.schedule(function() on_response(false, "Connection failed: " .. err) end)
            end
            return
        end
        
        local buffer = ""
        client:read_start(function(err_read, chunk)
            if err_read then
                timeout_timer:stop(); timeout_timer:close(); client:close()
                return
            end
            
            if chunk then
                buffer = buffer .. chunk
                
                while true do
                    if #buffer < 4 then break end
                    
                    -- Read 4-byte length
                    local b1, b2, b3, b4 = string.byte(buffer, 1, 4)
                    local data_len = b1 * 16777216 + b2 * 65536 + b3 * 256 + b4
                    
                    if #buffer < 4 + data_len then break end
                    
                    local data = buffer:sub(5, 4 + data_len)
                    buffer = buffer:sub(5 + data_len)
                    
                    local ok, decoded = pcall(vim.mpack.decode, data)
                    if ok and type(decoded) == "table" then
                        local msg_type = decoded[1]
                        if msg_type == 1 then -- Response: [1, msgid, error, result]
                            if decoded[2] == msgid then
                                timeout_timer:stop(); timeout_timer:close(); client:close()
                                if on_response then
                                    vim.schedule(function()
                                        local err_val = decoded[3]
                                        if err_val == nil or err_val == vim.NIL then
                                            on_response(true, decoded[4])
                                        else
                                            on_response(false, tostring(err_val))
                                        end
                                    end)
                                end
                                return
                            end
                        elseif msg_type == 2 then -- Notification: [2, method, params]
                            log.debug("RPC Notification received: method=%s", tostring(decoded[2]))
                            if on_notification then
                                vim.schedule(function() on_notification(decoded[2], decoded[3]) end)
                            end
                        end
                    end
                end
            else
                timeout_timer:stop(); timeout_timer:close(); client:close()
            end
        end)
        
        client:write(header .. encoded)
    end)
end

return M