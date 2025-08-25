-- lua/UNL/logging/init.lua (完全版 - マネージャーの公式窓口)
-- 各プラグインからのロガー要求を受け付け、内部のロガーマネージャーに中継する。

local loader = require("UNL.config.loader")
local manager = require("UNL.logging.logger")
local W_echo   = require("UNL.logging.writer.echo")
local W_file   = require("UNL.logging.writer.file")
local W_notify = require("UNL.logging.writer.notify")
local W_perf   = require("UNL.logging.writer.perf")

local M = {}

function M.setup(name, plugin_defaults, user_cfg)
  -- ★★★ 修正点1: loader.setup に name を渡す ★★★
  loader.setup(name, plugin_defaults, user_cfg)
  
  -- 2. ロガーインスタンスを作成
  return manager:create(name, {
    prefix = ("[%s]"):format(name:upper()),
    writers = { W_echo.new(), W_file.new(), W_notify.new(), W_perf.new() },
    -- ★★★ 修正点2: config_getter が name を使って設定を取得するようにする ★★★
    config_getter = function() return loader.get(name) end,
  })
end

function M.get(name)
  local logger = manager:get(name)
  if not logger then
    return {
      trace = function() end, debug = function() end, info = function() end,
      warn = function(...) vim.notify("WARN: Logger '"..name.."' not init. " .. vim.inspect(...), vim.log.levels.WARN) end,
      error = function(...) vim.notify("ERROR: Logger '"..name.."' not init. " .. vim.inspect(...), vim.log.levels.ERROR) end,
      perf = function() end,
    }
  end
  return logger
end

return M
