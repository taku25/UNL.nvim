-- lua/UNL/backend/dynamic_stack_picker/init.lua

local registry = require("UNL.backend.dynamic_stack_picker.registry")
local unl_config = require("UNL.config")
local unl_picker_factory = require("UNL.backend.factory.picker")

local provider_modules = {
  "UNL.backend.dynamic_stack_picker.provider.telescope",
  "UNL.backend.dynamic_stack_picker.provider.fzf_lua",
  "UNL.backend.dynamic_stack_picker.provider.snacks",
  "UNL.backend.dynamic_stack_picker.provider.dummy",
}

local M = {}
local loaded = false

function M.load_providers(spec)
  if loaded then return end
  unl_picker_factory.load_providers(registry, provider_modules, spec)
  loaded = true
end

--- @param spec table { title: string, start: function(push), on_submit: function(item), devicons_enabled: boolean }
function M.pick(spec)
  M.load_providers(spec)
  local log = require("UNL.logging").get(spec.logger_name or "UNL")

  if not spec.start or type(spec.start) ~= "function" then
    log.error("Dynamic Stack Picker: 'spec.start' function is required.")
    return
  end

  -- 1. spec.conf 自体に直接指定がある場合を最優先 (テストスクリプト等)
  -- 2. spec.conf.ui.dynamic_stack_picker ... と深くネストしている場合
  -- 3. UNLグローバルの設定
  local conf = (spec.conf and spec.conf.mode and spec.conf)
    or (spec.conf and spec.conf.ui and spec.conf.ui.dynamic_stack_picker)
    or (spec.conf and spec.conf.ui and spec.conf.ui.picker)
    or unl_config.get().ui.picker or {}

  unl_picker_factory.run_with_fallback({
    picker_type_name = "Dynamic Stack Picker",
    registry = registry,
    conf = conf,
    spec = spec,
    logger_name = spec.logger_name,
  })
end

return M
