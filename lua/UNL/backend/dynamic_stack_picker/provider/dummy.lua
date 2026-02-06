-- lua/UNL/backend/dynamic_stack_picker/provider/dummy.lua

local M = { name = "dummy" }

function M.available()
  return true
end

function M.run(spec)
  local log = require("UNL.logging").get(spec.logger_name or "UNL")
  log.warn("Dynamic Stack Picker: No UI provider available. Using dummy.")
  
  -- dummyは即座にstartを呼び出すが、pushしても何もしない
  if spec.start then
    spec.start(function() end)
  end
end

return M
