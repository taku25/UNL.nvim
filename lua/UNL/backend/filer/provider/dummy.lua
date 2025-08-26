local M = { name = "dummy" }

function M.available()
  -- dummyは常に利用可能
  return true
end

---
-- dummyプロバイダーの実行関数
-- @param spec table
function M.open(spec)
  local log = require("UNL.logging").get(spec.logger_name or "UNL")
  
  -- ユーザーに、サポートされているファイラーをインストールするように促す
  local error_msg = "UEP: No supported filer plugin (e.g., neo-tree.nvim) found."
  
  log.error(error_msg)
  vim.notify(error_msg, vim.log.levels.WARN)
end

M.run = M.open

return M
