-- lua/UNL/provider/init.lua
-- UNLプロバイダーシステムの公開API
-- UNL.nvimをハブとして、様々なプラグインがサービスを提供・利用できるようにする

local registry = require("UNL.provider.registry")
local config = require("UNL.config")
local log = require("UNL.logging").get("UNL.provider")

local M = {}

---
-- プロバイダーを登録する。主に他のプラグインから呼び出されることを想定。
-- @param spec table: 詳細は `registry.register` を参照
function M.register(spec)
  return registry.register(spec)
end

---
-- プロバイダーにリクエストを送り、レスポンスを期待する。
-- capabilityに最適なプロバイダーを見つけ、その `request` メソッドを呼び出す。
-- @param capability string: 要求する機能 (例: "vcs.get_status")
-- @param opts table: プロバイダーの `request` メソッドに渡す引数
-- @return boolean, any: `ok, result` を返す。プロバイダーが見つからない場合は `false, "No provider found"`
function M.request(capability, opts)
  log.debug("Request received for capability '%s'", capability)
  local conf = config.get("UNL") -- "UNL"名前空間の設定を取得
  local provider = registry.resolve(capability, conf)

  if not (provider and provider.impl and type(provider.impl.request) == "function") then
    log.warn("No provider with a 'request' method found for capability '%s'", capability)
    return false, "No provider found for " .. capability
  end

  log.info("Dispatching request for '%s' to provider '%s'", capability, provider.name)
  local ok, result = pcall(provider.impl.request, opts)

  if not ok then
    log.error("Provider '%s' failed on request for '%s': %s", provider.name, capability, tostring(result))
    return false, result -- resultにはエラーメッセージが含まれる
  end

  return true, result
end

---
-- 指定されたcapabilityを持つ全てのプロバイダーに通知を送る
-- これは投げっぱなし(fire-and-forget)の操作で、戻り値はない
-- @param capability string: 通知する対象の機能 (例: "project.file_changed")
-- @param opts table: 各プロバイダーの `notify` メソッドに渡す引数
function M.notify(capability, opts)
  log.debug("Notification received for capability '%s'", capability)
  local providers = registry.get_all(capability)

  if #providers == 0 then
    log.debug("No providers to notify for capability '%s'", capability)
    return
  end

  log.info("Dispatching notification for '%s' to %d provider(s)", capability, #providers)
  for _, provider in ipairs(providers) do
    if provider.impl and type(provider.impl.notify) == 'function' then
      -- 一つのプロバイダーのエラーが他を妨げないように schedule で実行
      vim.schedule(function()
        local ok, err = pcall(provider.impl.notify, opts)
        if not ok then
          log.error("Provider '%s' failed on notify for '%s': %s", provider.name, capability, tostring(err))
        end
      end)
    end
  end
end

return M
