local level = require("UNL.logging.level")

local M = {}

function M.new()
  local self = {}
  function self.write(msg_level, message, ctx)
    ctx = ctx or {}
    local meta = ctx.meta or {}
    if not meta.is_perf then return end
    local cfg = ctx.config or {}
    local perf_cfg = cfg.logging and cfg.logging.perf or {}
    if perf_cfg.enabled == false then return end
    local thr = level.parse(perf_cfg.level or "TRACE")
    if not level.visible(msg_level, thr) then return end
    local cat = meta.category or "default"
    local out = ("[PERF][%s] %s"):format(cat, message)
    vim.notify(out, msg_level)
  end
  return self
end

return M
