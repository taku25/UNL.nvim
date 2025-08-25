local level = require("UNL.logging.level")

local M = {}

function M.new()
  local self = {}
  function self.write(msg_level, message, ctx)
    ctx = ctx or {}
    local cfg = ctx.config or {}
    local thr = level.parse(cfg.logging and cfg.logging.echo and cfg.logging.echo.level)
    if not level.visible(msg_level, thr) then return end
    local hl = level.highlight(msg_level)
    local is_err = (msg_level >= vim.log.levels.WARN)
    vim.api.nvim_echo({ { message, hl } }, true, { err = is_err })
  end
  return self
end

return M
