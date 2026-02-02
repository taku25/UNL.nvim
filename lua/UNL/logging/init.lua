-- lua/UNL/logging/init.lua (完全版 - マネージャーの公式窓口)
-- 各プラグインからのロガー要求を受け付け、内部のロガーマネージャーに中継する。

local loader = require("UNL.config.loader")
local manager = require("UNL.logging.logger")
local W_echo   = require("UNL.logging.writer.echo")
local W_file   = require("UNL.logging.writer.file")
local W_notify = require("UNL.logging.writer.notify")
local W_perf   = require("UNL.logging.writer.perf")
local W_debug_buffer = require("UNL.logging.writer.debug_buffer")

local M = {}

function M.setup(name, plugin_defaults, user_cfg)
  -- ★★★ 修正点1: loader.setup に name を渡す ★★★
  loader.setup(name, plugin_defaults, user_cfg)
  
  -- 2. ロガーインスタンスを作成
  return manager:create(name, {
    prefix = ("[%s]"):format(name:upper()),
    writers = {
      W_echo.new(),
      W_file.new(),
      W_notify.new(),
      W_perf.new(),
      W_debug_buffer.new(),
    },
    -- ★★★ 修正点2: config_getter が name を使って設定を取得するようにする ★★★
    config_getter = function()
      return loader.get(name)
    end,
  })
end

function M.get(name)
  local logger = manager:get(name)
  if not logger then
    local seen = {}
    local function dummy_once(lvl_name, ...)
      local msg = vim.inspect(...)
      if seen[msg] then return end
      seen[msg] = true
      vim.notify(lvl_name .. ": Logger '"..name.."' not init. " .. msg, vim.log.levels[lvl_name] or vim.log.levels.WARN)
    end
    local function safe_inspect(...)
      local n = select("#", ...)
      if n == 0 then return "" end
      if n == 1 then
        local val = select(1, ...)
        return type(val) == "string" and val or vim.inspect(val)
      end
      return vim.inspect({...})
    end

    local function safe_format(...)
      local n = select("#", ...)
      if n == 0 then return "" end
      local first = select(1, ...)
      if type(first) ~= "string" then return safe_inspect(...) end
      
      local ok, res = pcall(string.format, ...)
      if ok then return res end
      return safe_inspect(...)
    end

    return {
      trace = function() end,
      debug = function() end,
      info = function(...)
        print("INFO: " .. safe_format(...))
      end,
      warn = function(...)
        print("WARN: " .. safe_format(...))
      end,
      error = function(...)
        print("ERROR: " .. safe_format(...))
      end,
      warn_once = function(...) dummy_once("WARN", ...) end,
      error_once = function(...) dummy_once("ERROR", ...) end,
      perf = function() end,
    }
  end
  return logger
end

function M.add_writer(name, writer)
  if not writer then return end
  return manager:add_writer(name, writer)
end

function M.dispatch_event(name, event_name, payload)
  manager:dispatch_event(name, event_name, payload)
end
return M
