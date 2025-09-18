-- lua/UNL/backend/dynamic_picker/init.lua

local registry = require("UNL.backend.dynamic_picker.registry")
local unl_config = require("UNL.config")

-- factoryは汎用的なのでそのまま再利用できる
local unl_picker_factory = require("UNL.backend.factory.picker")

local provider_modules = {
  "UNL.backend.dynamic_picker.provider.telescope",
  "UNL.backend.dynamic_picker.provider.fzf_lua",
  "UNL.backend.dynamic_picker.provider.dummy",
}

local M = {}
local loaded = false

function M.load_providers(spec)
  if loaded then return end
  -- factoryのプロバイダーローダーを再利用
  unl_picker_factory.load_providers(registry, provider_modules, spec)
  loaded = true
end

function M.pick(spec)
  M.load_providers(spec)
  local log = require("UNL.logging").get(spec.logger_name or "UNL")

  -- 1. dynamic_picker用の設定を取得 (なければ汎用pickerの設定を見る)
  local conf = (spec.conf and spec.conf.ui and spec.conf.ui.dynamic_picker)
    or (spec.conf and spec.conf.ui and spec.conf.ui.picker)
    or unl_config.get().ui.picker

  -- 2. 最適なプロバイダーを解決
  local provider, provider_name = registry.resolve(conf)

  if provider and provider.run then
    log.info("Dynamic Picker: Using provider '%s'", provider_name)
    local ok, err = pcall(provider.run, spec)
    if not ok then
      log.error("Dynamic Picker provider '%s' failed: %s", provider_name, tostring(err))
      -- 失敗時はdummyにフォールバック
      local dummy_provider = registry.get("dummy")
      if dummy_provider and dummy_provider.run then
        pcall(dummy_provider.run, spec)
      end
    end
  else
    log.error("Dynamic Picker: No available provider found.")
  end
end

return M
