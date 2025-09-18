-- lua/UNL/backend/dynamic_picker/registry.lua

local M = {}
local providers = {}

function M.register(provider_spec)
  if not (provider_spec and provider_spec.name) then
    local log = require("UNL.logging").get("UNL")
    log.warn("Dynamic Picker provider registration failed: missing name.")
    return
  end
  providers[provider_spec.name] = provider_spec
end

function M.get(name)
  return providers[name]
end

function M.resolve(conf)
  conf = conf or {}
  local mode = conf.mode or "auto"

  local chain = {}
  if mode == "auto" then
    chain = conf.prefer or { "telescope", "fzf_lua" }
  elseif mode ~= "none" and mode ~= "dummy" then
    chain = { mode }
  end
  table.insert(chain, "dummy") -- 最終フォールバック

  for _, name in ipairs(chain) do
    local provider = providers[name]
    if provider then
      local ok, is_available = pcall(provider.available)
      if ok and is_available then
        return provider, name
      end
    end
  end

  return providers["dummy"], "dummy"
end

function M._reset()
  providers = {}
end

return M
