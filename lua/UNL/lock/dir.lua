-- lua/UNL/lock/dir.lua
-- Directory-based lock utility.
-- Creates <base>/<name>.lock directory atomically to achieve mutual exclusion.
--
-- API:
--   local dir_lock = require("UNL.lock.dir")
--   local handle, err = dir_lock.acquire({
--       base_dir = "/abs/path/to/cache",
--       name = "refresh",          -- lock directory: refresh.lock
--       wait_ms = 0,               -- <=0: no wait
--       retry_interval_ms = 150,
--       stale_after_sec = 600,     -- stale threshold
--       force = false,             -- force takeover ignoring staleness
--       logger = logger,           -- optional (expects trace/info/warn/error)
--       meta = { any = "extra" },  -- optional metadata to write (JSON)
--     })
--   if not handle then ... end
--   -- critical section ...
--   handle:release()
--
-- Handle fields / methods:
--   handle.path         -- lock dir path
--   handle.meta_path    -- path to meta.json
--   handle.released     -- boolean
--   handle:release()    -- idempotent
--
-- Convenience:
--   dir_lock.with_lock(opts, function(handle)
--       ... critical section ...
--       return result
--   end)
--   -> returns ok, result_or_err
--
-- Notes:
--   - Atomicity relies on uv.fs_mkdir failing if directory exists.
--   - Stale judgement uses directory mtime (or meta.json mtime).
--   - Force takeover deletes existing lock unconditionally.
--   - No cross-host coordination (PID is informational only).
--

local uv = vim.loop
local fn = vim.fn
local fs = vim.fs

local M = {}

local function json_encode(tbl)
  if vim.json and vim.json.encode then
    return vim.json.encode(tbl)
  end
  return fn.json_encode(tbl)
end

local function get_stat(path)
  return uv.fs_stat(path)
end

local function now_sec()
  return os.time()
end

local function write_meta(dir, meta, logger)
  local meta_path = fs.joinpath(dir, "meta.json")
  meta = meta or {}
  local payload = {
    pid   = uv.os_getpid(),
    time  = now_sec(),
    nvim  = vim.version(),
    meta  = meta,
  }
  local ok, encoded = pcall(json_encode, payload)
  if not ok then
    if logger then logger.warn("lock meta encode failed: " .. tostring(encoded)) end
    return
  end
  local fd, err = uv.fs_open(meta_path, "w", 420) -- 0644
  if not fd then
    if logger then logger.warn("lock meta open failed: " .. tostring(err)) end
    return
  end
  uv.fs_write(fd, encoded, 0)
  uv.fs_close(fd)
end

local function remove_dir(dir)
  -- Best-effort remove meta then dir
  local meta = dir .. "/meta.json"
  pcall(uv.fs_unlink, meta)
  return uv.fs_rmdir(dir)
end

local function is_stale(stat, stale_after_sec)
  if not stat then return false end
  local mtime = stat.mtime
  local msec = 0
  if type(mtime) == "table" and mtime.sec then
    msec = mtime.sec
  elseif type(mtime) == "number" then
    msec = mtime
  else
    msec = now_sec()
  end
  return (now_sec() - msec) > stale_after_sec
end

--- Acquire a directory lock.
---@param opts table (see header)
---@return table|nil handle, string|nil err
function M.acquire(opts)
  opts = opts or {}
  local base_dir = opts.base_dir
  if not base_dir or base_dir == "" then
    return nil, "base_dir is required"
  end
  local name = opts.name or "lock"
  local lock_dir = fs.joinpath(base_dir, name .. ".lock")
  local logger = opts.logger
  local retry_interval_ms = opts.retry_interval_ms or 150
  local wait_ms = opts.wait_ms or 0
  local stale_after_sec = opts.stale_after_sec or 600
  local force = opts.force == true

  local start = uv.now()

  local function attempt()
    local ok, err = uv.fs_mkdir(lock_dir, 448) -- 0700
    if ok then
      write_meta(lock_dir, opts.meta, logger)
      return true, nil
    end
    return false, err
  end

  while true do
    local ok, err = attempt()
    if ok then
      if logger then logger.trace(("lock acquired: %s"):format(lock_dir)) end
      local handle = {
        path = lock_dir,
        meta_path = fs.joinpath(lock_dir, "meta.json"),
        released = false,
      }
      function handle:release()
        if self.released then return true end
        local r_ok, r_err = remove_dir(self.path)
        self.released = true
        return r_ok, r_err
      end
      return handle, nil
    else
      -- Exists or other error
      local stat = get_stat(lock_dir)
      if stat and (force or is_stale(stat, stale_after_sec)) then
        -- Try remove stale
        remove_dir(lock_dir)
        -- loop again
      else
        if (uv.now() - start) >= wait_ms then
          return nil, "busy"
        end
        uv.sleep(retry_interval_ms)
      end
    end
  end
end

--- Run fn inside lock, auto-release.
---@param opts table (same as acquire)
---@param fnc function
---@return boolean ok, any result_or_err
function M.with_lock(opts, fnc)
  local handle, err = M.acquire(opts)
  if not handle then
    return false, err
  end
  local ok, result = pcall(fnc, handle)
  handle:release()
  if not ok then
    return false, result
  end
  return true, result
end

return M
