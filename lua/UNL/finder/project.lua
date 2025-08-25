-- lua/UNL/finder/project.lua
-- Unreal Project ルート探索ユーティリティ (旧 pj.lua / 互換レイヤなし)
--
-- 公開 API:
--   project.find_project_root(start_path, opts) -> string|nil
--   project.find_project_file(start_path, opts) -> string|nil
--   project.find_project(start_path, opts) -> { root:string, uproject:string } | nil
--
-- 仕様:
--   *.uproject を含む最初の祖先ディレクトリを "Project Root" とみなす。
--   デフォルトでは最初に見つかった .uproject を採用。
--   複数存在時の選択戦略: opts.select_strategy
--     "first" (default) / "shortest" / "longest" / "alphabetical"
--
-- opts:
--   max_depth? integer
--   logger? { trace?, warn? }
--   debug? boolean
--   debug_files? boolean
--   debug_files_limit? integer
--   on_search_path?(path)
--   select_strategy? string
--   accept_pattern? string (default "%.uproject$")
--   filter?(filename, dir):boolean
--
-- 戻り値:
--   find_project_root: ディレクトリ or nil
--   find_project_file: フルパス or nil
--   find_project: { root=..., uproject=... } or nil
--
-- 内部実装: ancestor.find_up_forward に markers=function を渡す。

local ancestor = require("UNL.finder.ancestor")
local Path = require("UNL.path")

local M = {}

--------------------------------------------------
-- Logging helpers
--------------------------------------------------
local function trace(opts, msg)
  local l = opts and opts.logger
  if l and l.trace then l.trace(msg) end
end

local function warn(opts, msg)
  local l = opts and opts.logger
  if l and l.warn then l.warn(msg) end
end

--------------------------------------------------
-- Enumerate .uproject files in a directory
--------------------------------------------------
local function list_uprojects(dir, opts, accept_pattern)
  accept_pattern = accept_pattern or "%.uproject$"
  local ok, iter = pcall(vim.fs.dir, dir)
  if not ok or not iter then
    return {}
  end
  local out = {}
  for name, t in iter do
    if t == "file" and name:match(accept_pattern) then
      if (not opts.filter) or opts.filter(name, dir) then
        out[#out+1] = name
      end
    end
  end
  return out
end

--------------------------------------------------
-- Pick one candidate among multiple uproject files
--------------------------------------------------
local function pick_candidate(candidates, strategy)
  if #candidates == 0 then return nil end
  if #candidates == 1 then return candidates[1] end
  strategy = strategy or "first"

  if strategy == "first" then
    return candidates[1]
  end

  local indexed = {}
  for _, fname in ipairs(candidates) do
    local base = fname:gsub("%.uproject$", "")
    indexed[#indexed+1] = { fname = fname, base = base }
  end

  if strategy == "shortest" then
    table.sort(indexed, function(a, b)
      if #a.base == #b.base then return a.base < b.base end
      return #a.base < #b.base
    end)
  elseif strategy == "longest" then
    table.sort(indexed, function(a, b)
      if #a.base == #b.base then return a.base < b.base end
      return #a.base > #b.base
    end)
  elseif strategy == "alphabetical" then
    table.sort(indexed, function(a, b) return a.base < b.base end)
  else
    -- 未知 → first 相当
    return candidates[1]
  end

  return indexed[1].fname
end

--------------------------------------------------
-- Checker passed to ancestor.find_up_forward
--------------------------------------------------
local function make_checker(opts)
  return function(dir)
    local pattern = opts.accept_pattern or "%.uproject$"
    local candidates = list_uprojects(dir, opts, pattern)
    if #candidates == 0 then
      return nil
    end
    local picked = pick_candidate(candidates, opts.select_strategy)
    if picked then
      if opts.debug then
        trace(opts, ("[project] matched %s (%d candidates) in %s"):format(picked, #candidates, dir))
      end
      return dir
    end
    return nil
  end
end

--------------------------------------------------
-- Core locate function
--------------------------------------------------
local function locate(start_path, opts)
  opts = opts or {}
  start_path = Path.normalize(start_path)

  local checker = make_checker(opts)
  local root = ancestor.find_up(start_path, checker, {
    max_depth = opts.max_depth,
    logger = opts.logger,
    debug = opts.debug,
    debug_files = opts.debug_files,
    debug_files_limit = opts.debug_files_limit,
    on_search_path = opts.on_search_path,
  })
  if not root then
    return nil
  end

  local pattern = opts.accept_pattern or "%.uproject$"
  local candidates = list_uprojects(root, opts, pattern)
  local picked = pick_candidate(candidates, opts.select_strategy)
  if not picked then
    warn(opts, "[project] uproject file disappeared after detection: " .. root)
    return nil
  end
  local full_path = Path.normalize(vim.fs.joinpath(root, picked))
  return { root = Path.normalize(root), uproject = full_path }
end

--------------------------------------------------
-- Public API
--------------------------------------------------
function M.find_project(start_path, opts)
  return locate(start_path, opts)
end

function M.find_project_root(start_path, opts)
  local res = locate(start_path, opts)
  return res and res.root or nil
end

function M.find_project_file(start_path, opts)
  local res = locate(start_path, opts)
  return res and res.uproject or nil
end

function M.find_from_current_buffer(opts)
  local path = vim.api.nvim_buf_get_name(0)
  if path == "" then
    path = vim.loop.cwd()
  end
  return locate(path, opts)
end

return M
