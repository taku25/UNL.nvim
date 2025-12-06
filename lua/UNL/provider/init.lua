-- lua/UNL/provider/init.lua

local registry = require("UNL.provider.registry")
local config = require("UNL.config")

local M = {}

function M.register(spec)
  return registry.register(spec)
end

---
-- プロバイダーにリクエストを送る
-- @param capability string
-- @param opts table
-- @param on_complete function|nil (Optional) 非同期コールバック function(ok, result)
-- @return boolean, any (同期モード時のみ: ok, result)
function M.request(capability, opts, on_complete)
  opts = opts or {}

  local log = require("UNL.logging").get(opts.logger_name or "UNL")
  log.debug("Request received for capability '%s'", capability)
  
  local conf = config.get("UNL")
  local provider = registry.resolve(capability, conf)

  -- プロバイダーが見つからない場合
  if not (provider and provider.impl and type(provider.impl.request) == "function") then
    local err_msg = "No provider found for " .. capability
    log.debug(err_msg)
    
    if on_complete then
      on_complete(false, err_msg)
      return
    else
      return false, err_msg
    end
  end

  log.info("Dispatching request for '%s' to provider '%s'", capability, provider.name)

  -- ★★★ 非同期モード (コールバックあり) ★★★
  if on_complete then
    -- pcall で保護しつつ、実装側の request(opts, on_complete) を呼び出す
    local ok, err = pcall(provider.impl.request, opts, on_complete)
    
    if not ok then
      log.error("Provider '%s' crashed during async request '%s': %s", provider.name, capability, tostring(err))
      -- クラッシュした場合は、安全のため失敗としてコールバックを呼んでおく
      on_complete(false, "Provider crashed: " .. tostring(err))
    end
    return -- 非同期なので戻り値はなし
  end

  -- ★★★ 同期モード (コールバックなし / 既存互換) ★★★
  local ok, result = pcall(provider.impl.request, opts)

  if not ok then
    log.error("Provider '%s' failed on sync request '%s': %s", provider.name, capability, tostring(result))
    return false, result
  end

  return true, result
end

function M.notify(capability, opts)
  -- (変更なし)
  opts = opts or {}
  local log = require("UNL.logging").get(opts.logger_name or "UNL")
  local providers = registry.get_all(capability)
  for _, provider in ipairs(providers) do
    if provider.impl and type(provider.impl.notify) == 'function' then
      vim.schedule(function()
        pcall(provider.impl.notify, opts)
      end)
    end
  end
end

return M
