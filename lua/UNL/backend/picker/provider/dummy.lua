-- lua/UNL/backend/picker/provider/dummy.lua
-- 何もせず、エラーも出さない最終フォールバック
local M = { name = "dummy" }
function M.available() return true end
function M.run(spec)
  local unl_log = require("UNL.logging").get()
  unl_log.warn("Picker dummy provider was used. No UI was shown.")
  if spec.on_cancel then
    pcall(spec.on_cancel)
  end
end
return M
