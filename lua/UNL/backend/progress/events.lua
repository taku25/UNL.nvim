-- Event bus for progress
local M = {}
local global_hook = nil
local category_hooks = {} -- purpose -> fn

function M.set_global_hook(fn)
  global_hook = (type(fn) == "function") and fn or nil
end

function M.set_category_hook(purpose, fn)
  if type(fn) == "function" then
    category_hooks[purpose] = fn
  else
    category_hooks[purpose] = nil
  end
end

function M.emit(ev)
  ev.ts_ms = ev.ts_ms or (vim.loop.hrtime() / 1e6)
  if global_hook then pcall(global_hook, ev) end
  local cf = category_hooks[ev.purpose]
  if cf then pcall(cf, ev) end
end

return M
