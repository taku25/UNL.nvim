-- lua/UNL/provider/registry.lua
-- プロバイダーの中央登録・解決機構

local log = require("UNL.logging").get("UNL.provider")
local M = {}

-- Key: capability (string)
-- Value: 登録されたプロバイダー仕様のリスト
local providers_by_capability = {}

---
-- 新しいプロバイダーを登録する
-- @param spec table プロバイダーの仕様
--   - capability string: (必須) プロバイダーが提供する機能 (例: "vcs.status")
--   - name string: (必須) このプロバイダーのユニークな名前 (例: "unl-git.nvim")
--   - impl table: (必須) 機能を実装したモジュールまたはテーブル
--   - priority number: (任意, デフォルト 100) 競合解決のための優先度。数値が大きいほど優先。
function M.register(spec)
  if not (spec and spec.capability and spec.name and spec.impl) then
    log.error("Provider registration failed: 'capability', 'name', 'impl' are required.")
    return false
  end

  if not providers_by_capability[spec.capability] then
    providers_by_capability[spec.capability] = {}
  end

  -- 重複登録を避ける
  for _, existing in ipairs(providers_by_capability[spec.capability]) do
    if existing.name == spec.name then
      log.warn("Provider '%s' for capability '%s' is already registered. Overwriting.", spec.name, spec.capability)
      existing.impl = spec.impl
      existing.priority = spec.priority or 100
      return true
    end
  end

  table.insert(providers_by_capability[spec.capability], {
    name = spec.name,
    impl = spec.impl,
    priority = spec.priority or 100,
  })

  log.info("Registered provider '%s' for capability '%s'", spec.name, spec.capability)
  return true
end

---
-- 指定されたcapabilityに登録されている全てのプロバイダーを取得する
-- @param capability string
-- @return table プロバイダー仕様のリスト (見つからない場合は空テーブル)
function M.get_all(capability)
  return providers_by_capability[capability] or {}
end

---
-- ユーザー設定と優先度に基づいて、最適なプロバイダーを解決する
-- @param capability string
-- @param conf table ユーザー設定。 `conf.providers[capability] = "provider_name"` の形式を期待
-- @return table|nil 選択されたプロバイダーの仕様、見つからなければnil
function M.resolve(capability, conf)
  local candidates = M.get_all(capability)
  if #candidates == 0 then
    return nil
  end

  conf = conf or {}
  local preferred_provider_name = (conf.providers and conf.providers[capability])

  -- 1. ユーザーが明示的にプロバイダーを指定している場合
  if preferred_provider_name then
    for _, provider in ipairs(candidates) do
      if provider.name == preferred_provider_name then
        log.debug("Using user-preferred provider '%s' for capability '%s'", provider.name, capability)
        return provider
      end
    end
    log.warn("User-preferred provider '%s' for capability '%s' is not registered or available.", preferred_provider_name, capability)
  end

  -- 2. ユーザー指定がない場合、優先度でソートして最も高いものを選択
  table.sort(candidates, function(a, b)
    return a.priority > b.priority
  end)

  local best_provider = candidates[1]
  log.debug("Resolved provider '%s' for capability '%s' by priority (%d)", best_provider.name, capability, best_provider.priority)

  return best_provider
end

-- テスト用
function M._reset()
  providers_by_capability = {}
end

return M
