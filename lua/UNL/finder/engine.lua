-- lua/UNL/finder/engine.lua
-- Unreal Engine root resolver (opts.engine_override_path 版 / no legacy args).
--
-- Public API:
--   local engine_root, err = require("UNL.finder.engine").find_engine_root(project_file_path, {
--     engine_override_path = "D:/Unreal/UE_5.6", -- optional absolute path override
--     debug = true,                               -- optional debug tracing
--     logger = {                                  -- optional logger
--       trace = function(msg) print(msg) end,
--       warn  = function(msg) vim.notify(msg, vim.log.levels.WARN) end,
--     },
--   })
--
-- Resolution order:
--   1. opts.engine_override_path  (validate & return)
--   2. Read .uproject's EngineAssociation
--       - Absolute path  -> validate & return
--       - GUID {..}      -> helper script (find_engine.bat / find_engine.sh)
--       - Version X.Y    -> helper script
--       - Missing/empty  -> Try embedded project (walk parents for Engine/)
--   3. Failure => nil, error_message
--
-- Helper scripts (must output only the engine root path):
--   Windows: scripts/find_engine.bat
--   POSIX  : scripts/find_engine.sh
--
-- Engine structure check: presence of <root>/Engine/ directory.
--
-- Note: This module intentionally keeps heuristics conservative; extend as needed.

local M = {}

local fn  = vim.fn
local fs  = vim.fs
local uv  = vim.loop
local sep = package.config:sub(1,1)

-- ---------- Logging ----------
local function trace(opts, msg)
  if opts and opts.debug and opts.logger and opts.logger.trace then
    opts.logger.trace("[engine] " .. msg)
  end
end

local function warn(opts, msg)
  if opts and opts.logger and opts.logger.warn then
    opts.logger.warn("[engine] " .. msg)
  end
end

-- ---------- Platform / path helpers ----------
local function is_windows()
  return (uv.os_uname().version or ""):match("Windows") ~= nil
end

local function normpath(p)
  if not p or p == "" then return p end
  local ok, normalized = pcall(function()
    if fs.normalize then
      return fs.normalize(p)
    end
    return fn.fnamemodify(p, ":p")
  end)
  if ok and normalized and normalized ~= "" then
    return normalized:gsub(sep.."+$", "")
  end
  return p
end

local function join(...)
  return table.concat({ ... }, sep)
end

local function path_exists_dir(p)
  return p and p ~= "" and fn.isdirectory(p) == 1
end

local function is_absolute(path)
  if not path or path == "" then return false end
  if path:match("^[A-Za-z]:[\\/].") then return true end -- drive
  if path:match("^[\\/][\\/]")       then return true end -- UNC
  if path:sub(1,1) == "/"            then return true end -- POSIX
  return false
end

local function have_engine_structure(root)
  return path_exists_dir(root) and fn.isdirectory(join(root, "Engine")) == 1
end

-- Ascend parents to detect embedded engine (project inside Engine tree)
local function find_embedded_engine(start, opts)
  if not start or start == "" then return nil end
  local dir = normpath(start)
  if dir:lower():match("%.uproject$") then
    dir = fn.fnamemodify(dir, ":h")
  end
  local guard = 25
  while dir and dir ~= "" and guard > 0 do
    guard = guard - 1
    if have_engine_structure(dir) then
      trace(opts, "embedded engine root: " .. dir)
      return dir
    end
    local parent = fn.fnamemodify(dir, ":h")
    if parent == dir then break end
    dir = parent
  end
  return nil
end

-- Read EngineAssociation from .uproject
local function read_engine_association(project_file_path)
  if not project_file_path or project_file_path == "" then
    return nil, "uproject path not provided"
  end
  if project_file_path:sub(-9):lower() ~= ".uproject" then
    return nil, "not a .uproject file: " .. project_file_path
  end
  if fn.filereadable(project_file_path) ~= 1 then
    return nil, "uproject file not readable: " .. project_file_path
  end
  local ok, lines = pcall(fn.readfile, project_file_path)
  if not ok then
    return nil, "failed to read uproject: " .. tostring(lines)
  end
  local content = table.concat(lines, "\n")
  local decode_ok, data = pcall(fn.json_decode, content)
  if not decode_ok or type(data) ~= "table" then
    return nil, "failed to parse uproject JSON"
  end
  local assoc = data.EngineAssociation
  if assoc == nil or tostring(assoc) == "" then
    return nil, nil
  end
  return tostring(assoc), nil
end

-- Find plugin (scripts) root heuristically
local plugin_root_path -- このモジュール内でのみ有効なキャッシュ変数

local function find_plugin_root()
  if plugin_root_path then
    return plugin_root_path
  end

  for _, path in ipairs(vim.api.nvim_list_runtime_paths()) do
    if path:match("[/\\]UNL.nvim$") then
      plugin_root_path = path -- 見つけたらキャッシュに保存
      return path
    end
  end
  return nil
end

-- Run external helper (guid / version)
local function run_helper(assoc_type, value, opts)
  local root = find_plugin_root()
  if not root then
    return nil, "plugin root (scripts/) not found"
  end
  local script = is_windows()
      and join(root, "scripts", "find_engine.bat")
       or join(root, "scripts", "find_engine.sh")

  if fn.filereadable(script) ~= 1 then
    return nil, "helper script missing: " .. script
  end
  if not is_windows() then
    pcall(function() uv.fs_chmod(script, tonumber("755", 8)) end)
  end

  trace(opts, ("exec helper: %s %s %s"):format(script, assoc_type, value))
  local out = fn.system({ script, assoc_type, value })
  local code = vim.v.shell_error
  trace(opts, ("helper exit=%d raw=%q"):format(code, out))
  if code ~= 0 then
    return nil, ("helper failed exit=%d"):format(code)
  end
  out = vim.trim(out or "")
  if out == "" then
    return nil, "helper returned empty output"
  end
  if not have_engine_structure(out) then
    return nil, "helper path invalid: " .. out
  end
  return normpath(out), nil
end

-- Classify EngineAssociation
local function classify_association(association)
  if not association or association == "" then return "unkown" end

  -- 1. Check for GUID first (most specific)
  if association:match("^%b{}$") and association:match("^{[%x%-]+}$") then
    return "guid"
  end

  -- 2. Check for an absolute path
  if is_absolute(association) then
    return "path"
  end

  -- 3. If it's not a GUID and not a path, treat it as a
  --    Build ID / Custom Version string (like "5.3" or "UEQ-5.5.3")
  --    to be passed to the helper script.
  return "version"
end


-- Public API
-- opts = {
--   engine_override_path = "...", -- optional
--   debug = true/false,
--   logger = { trace = fn, warn = fn },
-- }
function M.find_engine_root(project_file_path, opts)
  opts = opts or {}

  -- 1. Override
  local override_path = opts.engine_override_path
  if override_path and override_path ~= "" then
    trace(opts, "override path: " .. override_path)
    if have_engine_structure(override_path) then
      return normpath(override_path), nil
    end
    return nil, "override path invalid (missing Engine): " .. override_path
  end

  -- 2. Read EngineAssociation
  local association, read_err = read_engine_association(project_file_path)
  if read_err then
    return nil, read_err
  end

  if not association or association == "" then
    trace(opts, "no EngineAssociation -> embedded scan")
    local embedded = find_embedded_engine(project_file_path, opts)
    if embedded then
      return normpath(embedded), nil
    end
    return nil, "no EngineAssociation and no embedded engine root"
  end

  trace(opts, "EngineAssociation=" .. association)
  local assoc_type = classify_association(association)
  if not assoc_type or assoc_type == "unknown" then
    return nil, "unknown EngineAssociation format: " .. association
  end

  if assoc_type == "path" then
    if have_engine_structure(association) then
      trace(opts, "association path valid")
      return normpath(association), nil
    end
    return nil, "EngineAssociation path invalid: " .. association
  end

  -- guid / version via helper
  local engine_root, herr = run_helper(assoc_type, association, opts)
  if engine_root then
    return engine_root, nil
  end
  warn(opts, "helper failure: " .. tostring(herr))
  return nil, herr
end

return M
