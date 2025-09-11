-- lua/UNL/backend/find_picker/init.lua

local registry = require("UNL.backend.find_picker.registry")
local unl_config = require("UNL.config")
local unl_picker_factory = require("UNL.backend.factory.picker")

local provider_modules = {
  "UNL.backend.find_picker.provider.telescope",
  "UNL.backend.find_picker.provider.fzf_lua",
  "UNL.backend.find_picker.provider.dummy",
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
  
  -- 1. 設定を取得する (これは変更なし)
  local conf = spec.conf.ui.find_picker or unl_config.get("UNL").ui.find_picker
  
  -- 2. (削除) prefer_chain をここで計算する必要はなくなった
  -- local prefer_chain = conf.prefer or { "telescope", "fzf-lua" }

  -- 3. (変更) factory には conf オブジェクトをそのまま渡す
  unl_picker_factory.run_with_fallback({
    picker_type_name = "Find Picker",
    registry = registry,
    conf = conf, -- <<< prefer_chain の代わりに conf を渡す
    spec = spec,
    logger_name = spec.logger_name or "UNL.find_picker",
  })
end

return M
