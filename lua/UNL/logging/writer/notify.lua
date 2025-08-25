local level = require("UNL.logging.level")

local M = {}

function M.new()
  local self = {}
  function self.write(msg_level, message, ctx)
    ctx = ctx or {}
    local cfg = ctx.config or {}
    local ncfg = cfg.logging and cfg.logging.notify or {}
    local thr = level.parse(ncfg.level)
    if not level.visible(msg_level, thr) then return end
    local prefix = ncfg.prefix or ""
    if prefix ~= "" then
      message = ("%s %s"):format(prefix, message)
    end
    vim.notify(message, msg_level)
  end
  return self
end

return M
