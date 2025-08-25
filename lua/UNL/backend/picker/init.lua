-- lua/UNL/backend/picker/init.lua

local registry = require("UNL.backend.picker.registry")
local unl_config = require("UNL.config")

-- プロバイダモジュールをロードしてレジストリに登録する
local provider_modules = {
  "UNL.backend.picker.provider.telescope",
  "UNL.backend.picker.provider.fzf_lua",
  "UNL.backend.picker.provider.native",
  "UNL.backend.picker.provider.dummy",
}

local M = {}

local loaded = false
-- ★★★ 初期化処理を、外部から呼び出せる関数にラップする ★★★
function M.load_providers()
  if loaded then return end
  
  -- loggerは、この関数が呼ばれる時には確実に初期化されている
  local log = require("UNL.logging").get("UNL")

  for _, mod_name in ipairs(provider_modules) do
    local ok, provider = pcall(require, mod_name)
    if ok and provider.name then
      registry.register(provider)
    else
      log.debug("Failed to load picker provider: %s", mod_name)
    end
  end
  loaded = true
  return true
end
---
-- 汎用ピッカーを実行する
-- @param kind string データの種類 (例: "project", "file_location")
-- @param spec table ピッカーの仕様 { title, items, on_submit, ... }
--
function M.pick(spec)
  -- 渡されたロガーがあればそれを使う。なければ"UNL"のロガーをデフォルトで使う
  local log = require("UNL.logging").get(spec.logger_name or "UNL")
  local conf = spec.conf.ui.picker or unl_config.get("UNL").ui.picker

  local provider, provider_name = registry.resolve(conf, { kind = spec.kind })
  -- print(spec)
  
  if provider then
    log.info("Picker: Using provider '%s' for kind '%s'", provider_name, spec.kind)
    local ok, err = pcall(provider.run, spec) -- specをそのまま渡す
    if not ok then
      log.error("Picker provider '%s' failed: %s", provider_name, tostring(err))
      registry.get("dummy").run(spec)
    end
  else
    log.error("Picker: No available provider found.")
  end
end

-- テスト用のヘルパー
M._registry = registry

return M
