local M = {}
local fn = vim.fn

local ok_cache_core, cache_core = pcall(require, "UNL.cache.core")

local state = {
  inited = false,
  path = nil,
  dir = nil,
  conf_hash = nil,
}

local function hash_conf(conf)
  local parts = {}
  for k, v in pairs(conf or {}) do
    if type(v) ~= "function" and type(v) ~= "table" then
      parts[#parts+1] = k .. "=" .. tostring(v)
    end
  end
  table.sort(parts)
  return table.concat(parts, "&")
end

local function ensure_dir(path)
  if fn.isdirectory(path) ~= 1 then
    pcall(fn.mkdir, path, "p")
  end
end

local function rotate_if_needed(conf)
  local max_kb = conf.progress_log_max_kb or 256
  if max_kb <= 0 or not state.path then return end
  local f = io.open(state.path, "r")
  if not f then return end
  local sz = f:seek("end")
  f:close()
  if not sz or sz / 1024 <= max_kb then return end

  local rotate = conf.progress_log_rotate or 2
  for i = rotate, 1, -1 do
    local old  = state.path .. "." .. i
    local prev = (i == 1) and state.path or (state.path .. "." .. (i - 1))
    if fn.filereadable(prev) == 1 then
      pcall(function()
        if fn.filereadable(old) == 1 or fn.isdirectory(old) == 1 then
          fn.delete(old)
        end
        fn.rename(prev, old)
      end)
    end
  end
end

local function write_line(line)
  local f = io.open(state.path, "a")
  if not f then return end
  f:write(line)
  if line:sub(-1) ~= "\n" then f:write("\n") end
  f:close()
end

local function iso_timestamp(conf)
  local fmt = conf.progress_log_time_format or "%Y-%m-%dT%H:%M:%S"
  return os.date(fmt)
end

function M.init(conf)
  if not ok_cache_core then return end
  local base_dir = cache_core.get_cache_dir(conf)
  local log_dir  = conf.progress_log_dir or (base_dir .. "/logs")
  ensure_dir(log_dir)

  local fname = conf.progress_log_filename or "progress.log"
  state.path = log_dir .. "/" .. fname
  state.dir = log_dir
  state.inited = true
  state.conf_hash = hash_conf(conf)

  rotate_if_needed(conf)
  write_line(string.format("# --- progress logger start (%s) ---", iso_timestamp(conf)))
end

local function serialize_event(ev)
  local parts = {}
  for k, v in pairs(ev) do
    local vt = type(v)
    if vt == "number" or vt == "string" or vt == "boolean" then
      parts[#parts+1] = k .. "=" .. tostring(v)
    end
  end
  table.sort(parts)
  return table.concat(parts, " ")
end

function M.append(conf, ev)
  if not state.inited then
    local cur_hash = hash_conf(conf)
    if state.conf_hash ~= cur_hash then
      M.init(conf)
    end
  end
  if not state.inited then return end

  rotate_if_needed(conf)

  local line = string.format(
    "%s kind=%s %s",
    iso_timestamp(conf),
    tostring(ev.phase or ev.kind or "?"),
    serialize_event(ev)
  )
  write_line(line)

  if ev.phase == "finish" or ev.kind == "finish" then
    write_line("# --- progress logger finish ---")
  end
end

return M
