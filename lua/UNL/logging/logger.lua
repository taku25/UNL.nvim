-- lua/UNL/logging/logger.lua (完全版 - ロガーマネージャー)
-- このモジュールは、名前付きロガーインスタンスを複数管理する責務を持つ。

local level = require("UNL.logging.level")

local function create_logger_instance(spec)
  spec = spec or {}
  local perf_cache = { patterns = {}, hash = nil }
  local function compile_perf(conf, cache)
    local perf = conf.logging and conf.logging.perf or {}
    if not perf.enabled then
      cache.patterns = {}
      return
    end
    local pats = perf.patterns or { ".*" }
    local h = table.concat(pats, "|")
    if h == cache.hash then
      return
    end
    cache.hash = h; cache.patterns = {}
    for _, p in ipairs(pats) do
      if pcall(function() string.find("", p) end) then cache.patterns[#cache.patterns + 1] = p end
    end
  end
 local function perf_match(category, cache)
    if not cache.patterns or #cache.patterns == 0 then
      return false
    end
    for _, p in ipairs(cache.patterns) do
      local ok, result = pcall(string.find, category, p)
      -- pcallが成功(ok)し、かつstring.findの結果(result)がnilでないことを確認する
      if ok and result then
        return true
      end
    end
    return false
  end
  local function dispatch(lvl, msg, meta)
    local cfg = spec.config_getter()
    local ts = os.date("!%Y-%m-%dT%H:%M:%S")
    local m = meta or {}
    m.timestamp = m.timestamp or ts
    m.level_name = m.level_name or level.name(lvl)
    m.prefix = spec.prefix
    if m.category == nil then
      m.category = (m.is_perf and "perf") or "general"
    end
    local final = spec.prefix and spec.prefix ~= "" and (spec.prefix .. " " .. msg) or msg
    for _, w in ipairs(spec.writers or {}) do
      -- pcall(function() w.write(lvl, final, { config = cfg, meta = m }) end)
      pcall(w.write, lvl, final, { config = cfg, meta = m })
    end
  end
  local L = {}
  L._spec = spec
  function L.perf(category, fmt, ...)
    local cfg = spec.config_getter()
    compile_perf(cfg, perf_cache)
    local perf_cfg = cfg.logging and cfg.logging.perf or {}
    if not perf_cfg.enabled then
      return
    end
    category = (type(category) == "string" and category ~= "") and category or "default"
    if not perf_match(category, perf_cache) then
      return
    end
    local msg = fmt
    if select("#", ...) > 0 then
      if pcall(string.format, fmt, ...) then
        msg = string.format(fmt, ...)
      end
    end
    dispatch(level.parse(perf_cfg.level or "TRACE"), msg, { is_perf = true, category = category })
  end
  local function make_fn(lvl)
    return function(fmt, ...)
      local msg = fmt; if select("#", ...) > 0 then
      if pcall(string.format, fmt, ...) then
        msg = string.format(fmt, ...)
        end
      end
      dispatch(lvl, msg, {})
    end
  end
  L.trace = make_fn(vim.log.levels.TRACE)
  L.debug = make_fn(vim.log.levels.DEBUG)
  L.info = make_fn(vim.log.levels.INFO)
  L.warn = make_fn(vim.log.levels.WARN)
  L.error = make_fn(vim.log.levels.ERROR)

  local seen_messages = {}
  local function make_once_fn(lvl)
    return function(fmt, ...)
      local msg = fmt; if select("#", ...) > 0 then
        if pcall(string.format, fmt, ...) then
          msg = string.format(fmt, ...)
        end
      end
      if seen_messages[msg] then return end
      seen_messages[msg] = true
      dispatch(lvl, msg, {})
    end
  end
  L.warn_once = make_once_fn(vim.log.levels.WARN)
  L.error_once = make_once_fn(vim.log.levels.ERROR)

  return L
end

local Manager = {}
Manager.__index = Manager
function Manager:new()
  return setmetatable({ _loggers = {} }, Manager)
end
function Manager:create(name, spec)
  if self._loggers[name] then
    return self._loggers[name]
  end
  self._loggers[name] = create_logger_instance(spec)
  return self._loggers[name]
end

function Manager:get(name)
  return self._loggers[name]
end

function Manager:add_writer(name, writer)
  local logger = self:get(name)
  if not logger then
    return false, "Logger not found: " .. tostring(name)
  end
  table.insert(logger._spec.writers, writer)
  return true
end

function Manager:_reset()
  self._loggers = {}
end

function Manager:dispatch_event(name, event_name, payload)
  local logger = self:get(name)
  if not (logger and logger._spec and logger._spec.writers) then
    return
  end
  
  -- 登録されている全てのwriterをループ
  for _, writer in ipairs(logger._spec.writers) do
    -- もしwriterがそのイベント名の関数を持っていれば、それを呼び出す
    if type(writer[event_name]) == "function" then
      -- pcallで安全に実行
      pcall(writer[event_name], writer, payload)
    end
  end
end
return Manager:new()
