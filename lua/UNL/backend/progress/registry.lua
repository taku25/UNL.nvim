-- Provider registry & resolution
local M = {}

local providers = {}     -- name -> spec
local ordered_cache = {} -- category -> sorted list (現在未使用 / 将来拡張用)

-- Customizable auto chain (refresh)
local custom_auto_chain_refresh = nil

function M.register(spec)
  assert(spec and spec.name, "provider spec.name required")
  providers[spec.name] = spec
  ordered_cache = {}
end

function M.get(name) return providers[name] end
function M.has(name) return providers[name] ~= nil end
function M.providers_list()
  local out = {}
  for k, v in pairs(providers) do
    out[#out+1] = { name = k, category = v.category, weight = v.weight, capabilities = v.capabilities }
  end
  table.sort(out, function(a,b) return a.name < b.name end)
  return out
end

function M._reset()
  for k in pairs(providers) do providers[k] = nil end
  for k in pairs(ordered_cache) do ordered_cache[k] = nil end
  custom_auto_chain_refresh = nil
end

-- Auto chain override API
function M.set_auto_chain_refresh(list)
  assert(type(list) == "table", "list must be table")
  custom_auto_chain_refresh = vim.deepcopy(list)
end

function M.reset_auto_chain_refresh()
  custom_auto_chain_refresh = nil
end

function M.get_auto_chain_refresh()
  return vim.deepcopy(custom_auto_chain_refresh)
end

local function provider_available(spec)
  if not spec then return false end
  if spec.detect == nil then return true end
  local ok, ret = pcall(spec.detect)
  return ok and ret == true
end

local function auto_chain_for_refresh()
  if custom_auto_chain_refresh then
    return custom_auto_chain_refresh
  end
  return { "fidget", "window", "notify", "dummy" }
end

local function capabilities_match(spec, require_all, require_any)
  if (not require_all) and (not require_any) then return true end
  local caps = spec.capabilities or {}
  if require_all then
    for _, key in ipairs(require_all) do
      if not caps[key] then
        return false
      end
    end
  end
  if require_any then
    local ok = false
    for _, key in ipairs(require_any) do
      if caps[key] then
        ok = true
        break
      end
    end
    if not ok then return false end
  end
  return true
end

-- opts:
--   category (required)
--   ui: "auto" | "none" | name | { names }
--   prefer: { names } (優先列挙; ui 指定より優先)
--   disable: boolean
--   require_capabilities: { cap1, cap2 } (AND)
--   any_capabilities: { capA, capB } (OR)
function M.resolve(opts)
  local category = assert(opts.category, "opts.category required")
  if opts.disable then
    return providers["dummy"], "dummy"
  end

  local mode = opts.mode or "auto"


  local chain = {}
  if type(opts.prefer) == "table" and #opts.prefer > 0 then
    chain = vim.deepcopy(opts.prefer)
  elseif mode == "auto" then
    chain = auto_chain_for_refresh()
  elseif mode ~= "none" then
    chain = { mode }
  end

  local require_all = opts.require_capabilities
  local require_any = opts.any_capabilities

  for _, name in ipairs(chain) do
    local spec = providers[name]
    if spec
       and spec.category == category
       and provider_available(spec)
       and capabilities_match(spec, require_all, require_any)
    then
      return spec, name
    end
  end

  return providers["dummy"], "dummy"
end

return M
