-- Public facade: prefer require("UNL.config")
local loader = require("UNL.config.loader")

local M = {}

function M.setup(name, default_cfg, user_cfg)
  return loader.setup(name, default_cfg, user_cfg)
end

function M.get(name, start_path, override)
  return loader.get(name, start_path, override)
end

function M.reload(name, start_path)
  return loader.reload(name, start_path)
end

function M.reset_single(name)
  return loader.reset_single(name)
end

function M.diagnose(name, start_path)
  return loader.diagnose(name, start_path)
end

-- Backward path (explicit loader) still available:
M.loader = loader

return M
