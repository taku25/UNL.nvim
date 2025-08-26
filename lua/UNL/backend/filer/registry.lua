-- UNL/backend/filer/registry.lua (resolve関数を追加)
local M = {}

local providers = {}

function M.register(spec)
  if spec and spec.name then
    providers[spec.name] = spec
  end
end

function M.get(name)
  return providers[name]
end

---
-- 設定に基づいて最適な利用可能プロバイダーを解決する
-- @param conf table ユーザー設定のfiler部分
-- @return table|nil, string|nil provider_spec, provider_name
function M.resolve(conf)
  conf = conf or {}
  local prefer_chain = conf.prefer or { "nvim-tree", "neo-tree", "native", "dummy" }

  for _, name in ipairs(prefer_chain) do
    local provider = providers[name]
    if provider and provider.available() then
      return provider, name
    end
  end
  
  -- 見つからなければdummyを返すのが安全
  return providers["dummy"], "dummy"
end

return M
