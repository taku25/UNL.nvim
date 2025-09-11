-- lua/UNL/backend/picker/init.lua

local registry = require("UNL.backend.picker.registry")
local unl_config = require("UNL.config")
local unl_picker_factory = require("UNL.backend.factory.picker")

-- (変更) native プロバイダーもリストに含まれていることを確認
local provider_modules = {
  "UNL.backend.picker.provider.telescope",
  "UNL.backend.picker.provider.fzf_lua",
  "UNL.backend.picker.provider.native",
  "UNL.backend.picker.provider.dummy",
}

local M = {}
local loaded = false

function M.load_providers(spec)
  if loaded then return end
  unl_picker_factory.load_providers(registry, provider_modules, spec)
  loaded = true
end

function M.pick(spec)
  M.load_providers(spec)
  
  -- 1. picker用の設定を取得
  local conf = spec.conf.ui.picker or unl_config.get("UNL").ui.picker
  
  -- 2. factoryに設定オブジェクトをそのまま渡す
  unl_picker_factory.run_with_fallback({
    picker_type_name = "Picker", -- ログ用の名前
    registry = registry,
    conf = conf,
    spec = spec,
    logger_name = spec.logger_name or "UNL.picker",
  })
end

-- (追加) テストや他のモジュールからレジストリにアクセスできるように
M._registry = registry

return M
