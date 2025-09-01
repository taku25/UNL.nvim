-- lua/UNL/backend/filer/init.lua
-- pickerのinit.luaと同じ設計思想に基づく、filerの公開API

local registry = require("UNL.backend.filer.registry")
local unl_config = require("UNL.config")

-- Filerプロバイダーのモジュールリスト
local provider_modules = {
  "UNL.backend.filer.provider.neo_tree",
  "UNL.backend.filer.provider.nvim_tree",
  "UNL.backend.filer.provider.native",
  "UNL.backend.filer.provider.dummy",
}

local M = {}

local loaded = false
---
-- Filerプロバイダーをロードしてレジストリに登録する
-- (picker.load_providers と同じ遅延ロードの仕組み)
function M.load_providers()
  if loaded then return end

  local log = require("UNL.logging").get("UNL")

  for _, mod_name in ipairs(provider_modules) do
    local ok, provider = pcall(require, mod_name)
    if ok and provider.name then
      registry.register(provider)
    else
      log.debug("Failed to load filer provider: %s", mod_name)
    end
  end
  loaded = true
  return true
end

---
-- 汎用ファイラーを開く
-- @param spec table ファイラーの仕様 { roots, prefer, ... }
function M.open(spec)
  M.load_providers()
  spec = spec or {}
  local log = require("UNL.logging").get(spec.logger_name or "UNL")
  
  -- pickerと同様に、呼び出し側(UEP)のconfを優先し、なければUNLのデフォルトを見る
  local conf = (spec.conf and spec.conf.ui and spec.conf.ui.filer)
    or unl_config.get().ui.filer

  -- registry.resolve を呼び出して、最適なプロバイダーを選択
  local provider, provider_name = registry.resolve(conf)
  
  if provider then
    log.info("Filer: Using provider '%s'", provider_name)
    -- spec をそのままプロバイダーの run (または open) 関数に渡す
    -- プロバイダーの実行関数名を "open" に統一するのが望ましい
    local run_function = provider.open or provider.run 
    if not run_function then
      log.error("Filer provider '%s' has no 'open' or 'run' function.", provider_name)
      -- dummy プロバイダーにも open/run がない場合に備えてガード
      local dummy_run = registry.get("dummy") and (registry.get("dummy").open or registry.get("dummy").run)
      if dummy_run then dummy_run(spec) end
      return
    end

    local ok, err = pcall(run_function, spec)
    if not ok then
      log.error("Filer provider '%s' failed: %s", provider_name, tostring(err))
      -- 失敗した場合はdummyプロバイダーにフォールバックする
      local dummy_run = registry.get("dummy") and (registry.get("dummy").open or registry.get("dummy").run)
      if dummy_run then dummy_run(spec) end
    end
  else
    log.error("Filer: No available provider found.")
  end
end

-- テスト用のヘルパー
M._registry = registry

return M
