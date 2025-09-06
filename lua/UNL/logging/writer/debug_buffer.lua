-- lua/UNL/logging/writer/debug_buffer.lua (無条件出力・最終完成版)

local M = {}

-- UNLのコアモジュールを遅延読み込みで利用
local level_utils -- = require("UNL.logging.level")

function M.new()
  local self = {}
  
  ---
  -- 全てのログメッセージを受け取り、デバッグビューワーに転送する
  -- @param msg_level number ログレベル (vim.log.levels.INFO など)
  -- @param message string ログメッセージ本体
  -- @param ctx table コンテキスト情報 { config, meta }
  function self.write(msg_level, message, ctx)
    -- 循環参照を避けるため、初回実行時にモジュールを読み込む
    local debug_log_viewer = require("UNL.backend.buf.debug_log")

    if not debug_log_viewer.is_open() then
      return
    end


    if not level_utils then
      level_utils = require("UNL.logging.level")
    end

    ctx = ctx or {}
    local lvl_name = level_utils.name(msg_level)
    local final_line = string.format("[%-5s] %s", lvl_name, message)
    
    pcall(debug_log_viewer.add_line, final_line)
  end
  
  return self
end

return M
