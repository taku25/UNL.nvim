-- Shallow schema validator (sanitizes in-place; collects errors)
local M = {}

local function expect(tbl, key, typ, default, errors, path)
  local v = tbl[key]
  if v == nil then
    tbl[key] = default
    return
  end
  if type(v) ~= typ then
    errors[#errors+1] = ("%s expected %s got %s"):format(path .. key, typ, type(v))
    tbl[key] = default
  end
end

local function sanitize_weights(wtbl, default, errors, path)
  if wtbl == nil then return default end
  if type(wtbl) ~= "table" then
    errors[#errors+1] = (path .. "weights must be table")
    return default
  end
  local clean = {}
  for k,v in pairs(wtbl) do
    if type(k) == "string" and type(v) == "number" and v >= 0 then
      clean[k] = v
    end
  end
  -- 空ならデフォルト
  local has = next(clean) ~= nil
  return has and clean or default
end

function M.validate(cfg)
  local errors = {}

  cfg.ui = cfg.ui or {}
  cfg.ui.progress = cfg.ui.progress or {}

  -- Progress
  expect(cfg.ui.progress, "mode", "string", "auto", errors, "ui.progress.")
  if cfg.ui.progress.enable == nil then
    cfg.ui.progress.enable = true
  end
  if cfg.ui.progress.allow_regression == nil then
    cfg.ui.progress.allow_regression = false
  end
  cfg.ui.progress.prefer = cfg.ui.progress.prefer or { "fidget", "window", "notify", "dummy" }

  expect(cfg.ui.progress, "window_max_lines", "number", 12, errors, "ui.progress.")
  expect(cfg.ui.progress, "window_width", "number", 52, errors, "ui.progress.")
  expect(cfg.ui.progress, "window_winblend", "number", 10, errors, "ui.progress.")
  expect(cfg.ui.progress, "throttle_ms", "number", 100, errors, "ui.progress.")
  expect(cfg.ui.progress, "title", "string", "UEP Refresh", errors, "ui.progress.")
  cfg.ui.progress.weights = sanitize_weights(
    cfg.ui.progress.weights,
    { scan=0.1, direct=0.55, transitive=0.3, finalize=0.05 },
    errors,
    "ui.progress."
  )

  -- Picker (既存)
  cfg.ui.picker = cfg.ui.picker or {}
  cfg.ui.picker.files = cfg.ui.picker.files or {}
  expect(cfg.ui.picker.files, "mode", "string", "auto", errors, "ui.picker.files.")
  cfg.ui.picker.files.prefer = cfg.ui.picker.files.prefer
    or { "telescope", "fzf_lua", "vim_select", "quickfix" }

  -- Logging (既存)
  cfg.logging = cfg.logging or {}
  cfg.logging.level = cfg.logging.level or "info"
  cfg.logging.echo = cfg.logging.echo or { level = "warn" }
  cfg.logging.notify = cfg.logging.notify or { level = "error", prefix = "[UNL]" }
  cfg.logging.file = cfg.logging.file or { enable = true, max_kb = 512, rotate = 3, filename = "unl.log" }
  cfg.logging.perf = cfg.logging.perf or { enabled = false, patterns = { "^refresh" }, level = "trace" }

  cfg.cache = cfg.cache or {}
  cfg.cache.dirname = cfg.cache.dirname or "UNL_cache"

  cfg.project = cfg.project or {}
  cfg.project.localrc_filename = cfg.project.localrc_filename or ".unlrc.json"
  if cfg.project.search_stop_at_home == nil then cfg.project.search_stop_at_home = true end
  if cfg.project.follow_symlink == nil then cfg.project.follow_symlink = true end

  return (#errors == 0), (#errors == 0) and cfg or errors
end

return M
