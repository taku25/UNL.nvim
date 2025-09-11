local M = { name = "dummy" }

function M.available()
  return true
end

function M.run(spec)
  local log = require("UNL.logging").get(spec.logger_name or "UNL")
  log.warn("Find Picker dummy provider was used. No UI shown. Install 'telescope.nvim' or 'fzf-lua'.")
  if spec.on_cancel then
    pcall(spec.on_cancel)
  end
end

return M
