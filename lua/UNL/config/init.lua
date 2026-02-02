-- Public facade: prefer require("UNL.config")
local loader = require("UNL.config.loader")

local M = {}

function M.setup(name, default_cfg, user_cfg)
  name = name or "UNL"
  return loader.setup(name, default_cfg, user_cfg)
end

function M.get(name, start_path, override)
  name = name or "UNL"
  return loader.get(name, start_path, override)
end

function M.reload(name, start_path)
  name = name or "UNL"
  return loader.reload(name, start_path)
end

function M.reset_single(name)
  name = name or "UNL"
  return loader.reset_single(name)
end

function M.diagnose(name, start_path)
  name = name or "UNL"
  return loader.diagnose(name, start_path)
end

-- Backward path (explicit loader) still available:
M.loader = loader

return M
