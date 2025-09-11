-- lua/UNL/backend/find_picker/registry.lua

local unl_log = require("UNL.logging").get("UNL")

local M = {}

local providers = {}

---
-- Register a new find_picker provider specification.
-- @param provider_spec table: Must contain a 'name' key.
function M.register(provider_spec)
  if not (provider_spec and provider_spec.name) then
    unl_log.warn("Find Picker provider registration failed: missing name.")
    return
  end
  providers[provider_spec.name] = provider_spec
end

---
-- Get a registered provider by name.
-- @param name string: The name of the provider (e.g., "telescope").
-- @return table|nil: The provider specification table or nil.
function M.get(name)
  return providers[name]
end

---
-- Resolve the best available provider based on user configuration.
-- @param conf table: The picker configuration table { mode, prefer }.
-- @return table|nil, string|nil: The provider spec and its name, or nil.
function M.resolve(conf)
  conf = conf or {}
  local mode = conf.mode or "auto"
  
  local chain = {}
  if mode == "auto" then
    -- (fzf-luaとtelescopeのどちらを優先するかは設定次第ですが、ここでは例として)
    chain = conf.prefer or { "telescope", "fzf_lua" }
  elseif mode ~= "none" and mode ~= "dummy" then
    chain = { mode }
  end
  
  -- Always add "dummy" as the final fallback.
  table.insert(chain, "dummy")

  for _, name in ipairs(chain) do
    local provider = providers[name]
    if provider then
      -- pcall for safety in case 'available' function errors out.
      local ok, is_available = pcall(provider.available)
      if ok and is_available then
        return provider, name
      end
    end
  end
  
  -- This should theoretically never be nil if dummy is registered.
  return providers["dummy"], "dummy"
end

---
-- (For testing purposes) Reset the internal registry.
function M._reset()
  providers = {}
end

return M
