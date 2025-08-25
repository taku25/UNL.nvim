-- lua/UNL/backend/picker/registry.lua

local unl_log = require("UNL.logging").get()

local M = {}

local providers = {}

function M.register(provider_spec)
  if not (provider_spec and provider_spec.name) then
    unl_log.warn("Picker provider registration failed: missing name.")
    return
  end
  providers[provider_spec.name] = provider_spec
end

function M.get(name)
  return providers[name]
end

---
-- 設定とコンテキストに基づいて最適なプロバイダを解決する
-- @param conf table { mode = "auto"|"telescope"|..., prefer = { ... } }
-- @param context table { kind = "project"|"file_location", ... }
-- @return table|nil provider_spec, string|nil provider_name
--
function M.resolve(conf, context)
  local mode = conf.mode or "auto"
  
  local chain = {}
  if mode == "auto" then
    chain = conf.prefer or { "telescope", "fzf_lua", "native" }
  elseif mode ~= "none" and mode ~= "dummy" then
    chain = { mode }
  end
  
  table.insert(chain, "dummy") -- 最終的なフォールバック

  for _, name in ipairs(chain) do
    local provider = providers[name]
    if provider then
      local ok, is_available = pcall(provider.available, context)
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
