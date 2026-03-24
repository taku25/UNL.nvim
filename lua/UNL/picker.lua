-- lua/UNL/picker.lua
-- Unified high-level Picker API for UNL.nvim
-- This module acts as the "Behavior" layer, handling high-level UI logic
-- like multiselect loops before delegating to backends.

local M = {}

local unl_picker_backend = require("UNL.backend.picker")
local logging = require("UNL.logging")

--- Open a unified picker.
--- @param spec table
---   - title (string)
---   - items (table) -- optional, shorthand for source.type = "static"
---   - source (table) -- { type = "static"|"grep"|"callback", ... }
---   - on_confirm (function) -- Called with the selected item(s)
---   - multiselect (boolean) -- If true, uses the looping "multi-picker" behavior
---   - devicons_enabled (boolean)
---   - preview_enabled (boolean)
function M.open(spec)
  local log = logging.get(spec.logger_name or "UNL")

  -- Backward compatibility for on_submit
  if not spec.on_confirm and spec.on_submit then
    spec.on_confirm = spec.on_submit
    spec.on_submit = nil
  end

  -- Support shorthand 'items'
  if not spec.source and spec.items then
    spec.source = { type = "static", items = spec.items }
  end

  -- Backward compatibility for find_picker's exec_cmd
  if not spec.source and spec.exec_cmd then
    spec.source = { type = "job", command = spec.exec_cmd }
  end

  -- Backward compatibility for dynamic_stack_picker's start
  if not spec.source and spec.start then
    spec.source = { type = "callback", fn = spec.start }
  end

  -- Default: Delegate to backend (handles "native" or "none")
  unl_picker_backend.pick(spec)
end

return M
