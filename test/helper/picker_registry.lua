-- test/helper/picker_registry.lua
-- picker.registry のテストを支援するヘルパー

local M = {}

function M.setup(providers_to_register)
  -- 1. requireキャッシュをクリアして、常にクリーンな状態でテストを開始
  package.loaded["UNL.backend.picker.registry"] = nil
  local registry = require("UNL.backend.picker.registry")

  -- 2. 偽物のプロバイダをレジストリに登録
  for _, provider in ipairs(providers_to_register) do
    registry.register(provider)
  end
  
  -- 3. テストで使えるように registry インスタンスを返す
  return { registry = registry }
end

function M.teardown(ctx)
  -- 何もしない
end

return M
