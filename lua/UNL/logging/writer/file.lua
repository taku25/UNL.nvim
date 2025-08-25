local level = require("UNL.logging.level")

local M = {}

local function ensure_dir(path)
  -- path には "ファイルパス" を渡す前提: 親ディレクトリを作成
  local dir = vim.fn.fnamemodify(path, ":h")
  if dir ~= "" and vim.fn.isdirectory(dir) == 0 then
    pcall(vim.fn.mkdir, dir, "p")
  end
end

local function rotate_if_needed(path, max_kb, rotate)
  if not max_kb or max_kb <= 0 then return end
  local f = io.open(path, "r")
  if not f then return end
  local size = f:seek("end"); f:close()
  if not size or (size / 1024) <= max_kb then return end
  rotate = rotate or 2
  for i = rotate, 1, -1 do
    local src = (i == 1) and path or (path .. "." .. (i - 1))
    local dst = path .. "." .. i
    if vim.fn.filereadable(src) == 1 then
      pcall(function()
        if vim.fn.filereadable(dst) == 1 then vim.fn.delete(dst) end
        vim.fn.rename(src, dst)
      end)
    end
  end
end

function M.new()
  local self = {}
  local log_path

  local function resolve_path(cfg)
    if log_path then return log_path end
    local file_cfg = cfg.logging and cfg.logging.file or {}
    local fname = file_cfg.filename or "unl.log"
    local cache_dir = vim.fs.joinpath(
      vim.fn.stdpath("cache"),
      (cfg.cache and cfg.cache.dirname) or "UNL_cache"
    )
    log_path = vim.fs.joinpath(cache_dir, fname)
    ensure_dir(log_path)  -- ここを cache_dir から log_path に変更
    return log_path
  end

  function self.write(msg_level, message, ctx)
    ctx = ctx or {}
    local cfg = ctx.config or {}
    local file_cfg = cfg.logging and cfg.logging.file or {}
    if not file_cfg.enable then return end

    local base_thr = level.parse(cfg.logging and cfg.logging.level)
    local file_thr = level.parse(file_cfg.level or (cfg.logging and cfg.logging.level))
    local thr = file_thr or base_thr
    if not level.visible(msg_level, thr) then return end

    local path = resolve_path(cfg)
    rotate_if_needed(path, file_cfg.max_kb, file_cfg.rotate)

    local ts = os.date("[%Y-%m-%d %H:%M:%S]")
    local lvl = level.name(msg_level)
    local line = ("%s %-5s %s"):format(ts, lvl, message)
    local ok, f = pcall(io.open, path, "a")
    if ok and f then
      f:write(line .. "\n")
      f:close()
    end
  end

  return self
end

return M
