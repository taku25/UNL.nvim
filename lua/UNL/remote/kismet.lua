-- UNL.nvim/lua/unl/remote/kismet.lua
--
-- Unreal Engine Remote Control APIと通信するための低レベルライブラリモジュール。
-- 呼び出し元で接続情報が指定されなかった場合、UNLのデフォルト設定にフォールバックする。

-- config.get()ではなく、直接defaultsをrequireすることで、循環参照のリスクを完全に避ける
-- このファイルはconfigモジュールから利用される可能性はないが、安全なパターンとして推奨される
local unl_defaults = require("UNL.config.defaults")

local M = {}

--- UEにコンソールコマンドをリモートで送信する
--
-- @param opts table | 以下のキーを持つテーブル:
--   - command (string, 必須): 実行したいコンソールコマンド。
--   - host (string, オプション): 接続先のホスト名。未指定の場合はUNLのデフォルト設定が使われる。
--   - port (number, オプション): 接続先のポート番号。未指定の場合はUNLのデフォルト設定が使われる。
--   - on_success (function, オプション): 成功時に呼び出されるコールバック。
--   - on_error (function, オプション): 失敗時に呼び出されるコールバック。
function M.execute(opts)
  opts = opts or {}
  local on_success = opts.on_success or function() end
  local on_error = opts.on_error or function() end

  if not opts.command or type(opts.command) ~= "string" or opts.command == "" then
    on_error("[UNL Remote] Invalid or empty command provided.")
    return
  end

  -- ★★★ ここがハイブリッドアプローチの心臓部 ★★★
  -- 1. optsに直接指定された値を最優先する
  -- 2. 指定されていなければ、UNLのデフォルト設定から取得する
  local host = opts.host or unl_defaults.remote.host
  local port = opts.port or unl_defaults.remote.port

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

  client:connect(host, port, function(connect_err)
    if connect_err then
      on_error(string.format("[UNL Remote] Connection failed to %s:%d.", host, port))
      client:close()
      return
    end

    local response_body_chunks = {} -- ★ 文字列結合ではなくテーブルに溜め込む
    client:read_start(function(read_err, data)
      if read_err then
        on_error("[UNL Remote] Error while reading response: " .. tostring(read_err))
        client:close()
        return
      end

      -- サーバーが接続を閉じた (通信完了)
      if not data then
        -- ★★★ ここが最重要修正点 ★★★
        -- すべてのチャンクを結合して、最終的な応答を組み立てる
        local final_response = table.concat(response_body_chunks)
        
        -- 最終的な応答を評価する
        if final_response:find("HTTP/1.1 200 OK") then
          on_success(final_response)
        else
          on_error("[UNL Remote] Request failed with response:\n" .. final_response)
        end
        
        client:close() -- 最後にクローズ
        return
      end
      
      -- データチャンクをテーブルに追加
      table.insert(response_body_chunks, data)
    end)

    client:write(http_request, function(write_err)
      if write_err then
        on_error("[UNL Remote] Error sending request: " .. tostring(write_err))
        client:close()
      end
    end)
  end)
end

return M
