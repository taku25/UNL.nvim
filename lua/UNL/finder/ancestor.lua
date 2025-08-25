-- lua/UNL/finder/ancestor.lua
-- 親方向に 1 ディレクトリずつ遡り、条件に一致する最初のディレクトリを返すユーティリティ。
--
-- 公開 API:
--   ancestor.find_up_forward(start_path, markers, opts)
--
-- markers:
--   string    : 単一 Lua パターン
--   table     : パターン配列 { "pat1", "pat2", ... }
--   function  : カスタムチェッカー (dir, original_markers, opts) -> string|nil
--               original_markers にはその function 自体が渡る
--   nil       : 何もマッチしない (結果は nil)
--
-- opts:
--   max_depth           : number (default 120)
--   on_search_path(path): function  各階層到達時に呼ばれる (pcall 保護)
--   logger              : { trace?, warn? }  (DI: trace / warn メソッド想定)
--   debug               : boolean  深さ/マッチログ出力
--   debug_files         : boolean  各階層でファイル一覧(一部)をログ
--   debug_files_limit   : number   (default 40)
--
-- 戻り値:
--   見つかったディレクトリ文字列 / 見つからなければ nil
--
-- 注意:
--   * ignore_case や キャッシュは未実装（将来 find_down_forward 実装時に検討）
--   * markers=function の場合は pcall で例外吸収し warn ログ
--   * パフォーマンス: 各階層でファイル列挙 (通常は浅いので十分)
--
-- 依存: Neovim 0.9+ (vim.fs, vim.uv)

local fs = vim.fs
local uv = vim.uv or vim.loop
local fn = vim.fn
local Path = require("UNL.path")

local M = {}

--------------------------------------------------
-- Logging helpers
--------------------------------------------------
local function log_trace(opts, msg)
  local l = opts and opts.logger
  if l and l.trace then l.trace(msg) end
end

local function log_warn(opts, msg)
  local l = opts and opts.logger
  if l and l.warn then l.warn(msg) end
end

--------------------------------------------------
-- File listing (files only)
--------------------------------------------------
local function list_files(dir, opts)
  -- Try vim.fs.dir first
  local ok, iter = pcall(fs.dir, dir)
  if ok and iter then
    local files = {}
    for name, t in iter do
      if t == "file" then
        files[#files+1] = name
      end
    end
    return files
  end

  -- Fallback: uv.fs_scandir
  local handle, err = uv.fs_scandir(dir)
  if not handle then
    log_warn(opts, "[ancestor] fs_scandir failed: " .. dir .. " err=" .. tostring(err))
    return {}
  end
  local out = {}
  while true do
    local name, typ = uv.fs_scandir_next(handle)
    if not name then break end
    if typ == "file" or typ == nil then
      out[#out+1] = name
    end
  end
  return out
end

local function is_root(path)
  if path == "/" then return true end
  if path:match("^%a:[/\\]?$") then return true end -- Windows drive root
  return false
end

--------------------------------------------------
-- Core ascend loop
--------------------------------------------------
local function ascend(start_path, step_func, opts)
  opts = opts or {}
  local current = fs.normalize(start_path)
  if fn.isdirectory(current) == 0 then
    current = fs.dirname(current)
  end

  local max_depth = opts.max_depth or 120
  local depth = 0
  local prev = ""

  while current and current ~= prev and depth <= max_depth do
    if opts.on_search_path then
      pcall(opts.on_search_path, current)
    end
    if opts.debug then
      log_trace(opts, ("[ancestor] depth=%d path=%s"):format(depth, current))
    end

    if opts.debug_files then
      local files = list_files(current, opts)
      table.sort(files)
      local limit = opts.debug_files_limit or 40
      local shown = {}
      for i = 1, math.min(#files, limit) do
        shown[#shown+1] = files[i]
      end
      local suffix = (#files > limit) and ("...(+" .. (#files - limit) .. ")") or ""
      log_trace(opts, ("[ancestor] files %s: %s %s"):format(current, table.concat(shown, ", "), suffix))
    end

    local hit = step_func(current)
    if hit then
      if opts.debug then
        log_trace(opts, "[ancestor] matched directory: " .. tostring(hit))
      end
      return hit
    end

    if is_root(current) then
      if opts.debug then
        log_trace(opts, "[ancestor] reached root: " .. current)
      end
      break
    end

    prev = current
    current = fs.dirname(current)
    depth = depth + 1
  end

  if opts.debug then
    log_trace(opts, "[ancestor] not found. start=" .. start_path)
  end
  return nil
end

--------------------------------------------------
-- Public API
--------------------------------------------------
function M.find_up(start_path, markers, opts)
  opts = opts or {}

  if not start_path or start_path == "" then
    if opts.debug then
      log_warn(opts, "[ancestor] empty start_path")
    end
    return nil
  end

  local original = start_path
  start_path = Path.normalize(start_path)

  -- ファイルなら親ディレクトリに切り替え
  local stat = vim.loop.fs_stat(start_path)
  if stat and stat.type == "file" then
    start_path = vim.fs.dirname(start_path)
  end

  if opts.debug and original ~= start_path then
    log_trace(opts, ("[ancestor] normalized start_path: %s -> %s"):format(original, start_path))
  end

  local mtype = type(markers)

  -- (以下は従来 find_up_forward の本体と同じ)
  if mtype == "function" then
    local step = function(dir)
      local ok, res = pcall(markers, dir, markers, opts)
      if not ok then
        log_warn(opts, "[ancestor] marker function error: " .. tostring(res))
        return nil
      end
      return res
    end
    return ascend(start_path, step, opts)
  end

  local patterns = {}
  if mtype == "string" then
    if markers ~= "" then patterns = { markers } end
  elseif mtype == "table" then
    for _, v in ipairs(markers) do
      if type(v) == "string" and v ~= "" then
        patterns[#patterns+1] = v
      end
    end
  elseif markers ~= nil then
    log_warn(opts, "[ancestor] unsupported markers type: " .. mtype)
  end

  if #patterns == 0 then
    if opts.debug then
      log_trace(opts, "[ancestor] no markers; nothing to search")
    end
    return nil
  end

  local step = function(dir)
    local files = list_files(dir, opts)
    for _, fname in ipairs(files) do
      for _, pat in ipairs(patterns) do
        if fname:match(pat) then
          if opts.debug then
            log_trace(opts, ("[ancestor] marker matched %s in %s"):format(fname, dir))
          end
          return dir
        end
      end
    end
    return nil
  end

  return ascend(start_path, step, opts)
end

--------------------------------------------------
-- Debug helper
--------------------------------------------------
function M._debug_list_files(dir)
  return (list_files(dir, {}))
end

return M
