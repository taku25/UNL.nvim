-- UNL.nvim/lua/unl/remote/kismet.lua (Fire and Forget版)

local unl_config = require("UNL.config")

local M = {}

function M.execute(opts)
  opts = opts or {}
  local on_success = opts.on_success or function() end
  local on_error = opts.on_error or function() end

  local conf = unl_config.get("UNL")
  -- 接続のタイムアウトは短い方が良い（例: 2秒）
  local timeout_ms = (conf and conf.remote and conf.remote.timeout) or 2000

  if not opts.command or type(opts.command) ~= "string" or opts.command == "" then
    on_error("[UNL Remote] Invalid or empty command provided.")
    return
  end

  local host = opts.host or (conf and conf.remote and conf.remote.host)
  local port = opts.port or (conf and conf.remote and conf.remote.port)

  -- ... (HTTPリクエストの準備は変更なし) ...
  local escaped_command = opts.command:gsub("\\", "\\\\"):gsub("\"", "\\\"")
  local object_path = "/Script/Engine.Default__KismetSystemLibrary"
  local function_name = "ExecuteConsoleCommand"
  local json_template = [[{"objectPath":"%s","functionName":"%s","parameters":{"WorldContextObject":null,"Command":"%s"},"generateTransaction":true}]]
  local json_body = string.format(json_template, object_path, function_name, escaped_command)
  local http_path = "/remote/object/call"
  local http_request_lines = {
    "PUT " .. http_path .. " HTTP/1.1", "Host: " .. host,
    "Content-Type: application/json", "Content-Length: " .. #json_body,
    "", json_body,
  }
  local http_request = table.concat(http_request_lines, "\r\n")

  local client = vim.loop.new_tcp()
  local timer = vim.loop.new_timer()
  local callback_called = false

  local function cleanup(err_msg, success_msg)
    if callback_called then return end
    callback_called = true

    if timer then
      timer:stop()
      timer:close()
      timer = nil
    end

    if client then
      client:close()
      client = nil
    end

    if err_msg then
      on_error(err_msg)
    else
      -- 成功メッセージはオプション
      on_success(success_msg)
    end
  end

  timer:start(timeout_ms, 0, function()
    cleanup(string.format("[UNL Remote] Connection/Write timed out after %dms.", timeout_ms))
  end)

  client:connect(host, port, function(connect_err)
    if connect_err then
      return cleanup(string.format("[UNL Remote] Connection failed to %s:%d.", host, port))
    end
    
    -- ★★★ ここからがFire and Forgetの核心 ★★★
    
    -- UEに応答を要求せず、データの書き込みだけを行う
    client:write(http_request, function(write_err)
      if write_err then
        -- 書き込みに失敗した場合のみエラーとする
        return cleanup("[UNL Remote] Error sending request: " .. tostring(write_err))
      end
      
      -- ★★★ 書き込みが成功した時点で、処理は成功とみなす ★★★
      -- UEからの応答は待たずに、即座に成功コールバックを呼び出し、接続を閉じる
      return cleanup(nil, "[UNL Remote] Command sent successfully (Fire and Forget).")
    end)

    -- client:read_start(...) の部分は完全に削除する
  end)
end

return M
