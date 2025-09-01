local unl_log      = require("UNL.logging")
local unl_defaults = require("UNL.config.defaults")


local M = {}

local initialized = false

function M.setup(user_config)
  if initialized then return M end -- 複数回呼ばれるのを防ぐ

  -- 1. ★★★ 最初にロガーと設定システムを完全に初期化する ★★★
  unl_log.setup("UNL", unl_defaults, user_config or {})
  
  -- 2. ★★★ 次に、他のサブシステムを初期化する ★★★
  -- これで、これらの関数が内部で unl_config.get() を呼んでも、
  -- 完全に準備ができた設定を読み込める
  -- 3. ★★★ 全ての準備が終わった最後に、ログを出力する ★★★
  local log = unl_log.get("UNL")
  if log then
    log.debug("UNL library setup complete.")
  end
  
  initialized = true
  return M
end

-- ファイルが初めてrequireされた時に、最低限のデフォルト設定で自動的に初期化を行う
if not initialized then
  M.setup({})
end

return M
