-- lua/UNL/backend/find_picker/init.lua

local registry = require("UNL.backend.find_picker.registry")
local unl_config = require("UNL.config")

local provider_modules = {
  "UNL.backend.find_picker.provider.telescope",
  "UNL.backend.find_picker.provider.fzf_lua",
  "UNL.backend.find_picker.provider.dummy",
}

local M = {}
local loaded = false

function M.load_providers()
  if loaded then return end
  local log = require("UNL.logging").get("UNL")
  for _, mod_name in ipairs(provider_modules) do
    local ok, provider = pcall(require, mod_name)
    if ok and provider.name then
      registry.register(provider)
    else
      log.debug("Failed to load find_picker provider: %s", mod_name)
    end
  end
  loaded = true
end

function M.pick(spec)
  M.load_providers()
  
  local log = require("UNL.logging").get(spec.logger_name or "UNL")
  
  -- ▼▼▼ ここからが変更箇所 ▼▼▼

  -- 1. UCMなど、呼び出し元のプラグインのコンフィグを取得
  local caller_conf = spec.conf or {}
  
  -- 2. find_picker用の設定を探す。なければUNLのデフォルト設定にフォールバック
  local find_picker_conf
  if caller_conf.ui and caller_conf.ui.find_picker then
    find_picker_conf = caller_conf.ui.find_picker
  else
    find_picker_conf = unl_config.get("UNL").ui.find_picker
  end

  -- 3. 取得した設定を使って、プロバイダーを解決する
  local provider, provider_name = registry.resolve(find_picker_conf)
  
  -- ▲▲▲ ここまでが変更箇所 ▲▲▲
  
  if provider then
    log.info("Find Picker: Using provider '%s'", provider_name)
    local ok, err = pcall(provider.run, spec)
    if not ok then
      log.error("Find Picker provider '%s' failed: %s", provider_name, tostring(err))
      registry.get("dummy").run(spec)
    end
  else
    log.error("Find Picker: No available provider found.")
  end
end

return M
