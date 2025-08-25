local registry = require("UNL.backend.progress.registry")
local events   = require("UNL.backend.progress.events")

local provider_modules = {
  "UNL.backend.progress.provider.fidget",
  "UNL.backend.progress.provider.window",
  "UNL.backend.progress.provider.notify",
  "UNL.backend.progress.provider.dummy",
}

local loaded = false
local function load_providers()
  if loaded then return end
  for _, mod in ipairs(provider_modules) do
    local ok, spec = pcall(require, mod)
    if ok and type(spec) == "table" and spec.name then
      if not registry.has(spec.name) then
        registry.register(spec)
      end
    else
      -- デバッグ出力: エラー理由を表示 (必要なくなったら削除)
      -- print(string.format("[progress:init] provider load failed: %s (%s)", mod, tostring(spec)))
    end
  end
  loaded = true
end

local function reset_providers()
  registry._reset()
  loaded = false 
end

load_providers()

local M = {}

function M.create_for_refresh(conf, opts)
  opts = opts or {}
  local ui_conf = conf.ui.progress or {}


  local spec, chosen = registry.resolve{
    category = "progress",
    mode       = ui_conf.mode,
    disable  = (ui_conf.enable == false),
    prefer   = ui_conf.prefer,
    context  = { purpose = "refresh" },
  }

  local inst = spec and spec.create and spec.create{
    enabled     = not (ui_conf.enable == false),
    weights     = ui_conf.weights,
    title       = opts.title or ui_conf.title or "UEP Refresh",
    client_name = opts.client_name or "UNL",
    throttle_ms = ui_conf.throttle_ms,
    window_progress_max_lines = ui_conf.window_max_lines,
    window_progress_width     = ui_conf.window_width,
    window_progress_winblend  = ui_conf.window_winblend,
  } or {
    open=function() end,
    update=function() end,
    finish=function() end,
    stage_define = function() end,
    stage_update = function() end }

  local W = {}
  function W:open() if inst.open then inst:open() end end

  -- 新規: multi-stage 用の透過メソッド (必要なければ削除可)
  function W:stage_define(name, total)
    events.emit{
      category="progress", purpose="refresh",
      phase="define", stage=name, total=total,
    }
    if inst.stage_define then inst:stage_define(name, total) end
  end
  function W:stage_update(name, done, msg)
    events.emit{
      category="progress", purpose="refresh",
      phase="stage_update", stage=name, done=done, message=msg,
    }
    if inst.stage_update then inst:stage_update(name, done, msg) end
  end

  function W:update(stage, message, uopts)
    events.emit{
      category="progress",
      purpose="refresh",
      stage  = stage,
      phase  = "update",
      message = message,
    }
    if inst.update then inst:update(stage, message, uopts) end
  end
  function W:finish(ok)
    events.emit{
      category="progress",
      purpose="refresh",
      phase="finish",
      ok=ok,
    }
    if inst.finish then inst:finish(ok) end
  end
  return W, chosen
end

M.events = events
M.registry = registry
M._load_providers = load_providers
M._reset_providers = reset_providers

return M
