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
  
  -- Handle multiselect behavior: "loop", "native", or "none"
  if spec.multiselect == "loop" then
    if spec.source and spec.source.type == "static" then
      return M.open_multi_loop(spec)
    end
  end

  -- Default: Delegate to backend (handles "native" or "none")
  unl_picker_backend.pick(spec)
end

-- Internal: Looping multi-selection behavior
function M.open_multi_loop(spec)
  local selected_items = {}
  local original_on_confirm = spec.on_confirm
  
  local function start_loop()
    local source_items = spec.source.items or {}
    local current_items = {}
    
    -- Add "[Done]" entry at the top
    table.insert(current_items, {
      label = "[Done] Finish selection",
      value = "__DONE__",
      icon = "✅",
    })
    
    -- Add available items, skipping already selected ones
    for _, item in ipairs(source_items) do
      local item_val = type(item) == "table" and (item.value or item) or item
      local already_selected = false
      for _, s in ipairs(selected_items) do
        if s == item_val then already_selected = true; break end
      end
      
      if not already_selected then
        table.insert(current_items, item)
      end
    end
    
    local loop_spec = vim.tbl_deep_extend("force", spec, {
      title = (spec.title or "Select") .. " (Selected: " .. #selected_items .. ")",
      source = { type = "static", items = current_items },
      multiselect = false, -- Single pick per loop
      on_confirm = function(selection)
        if not selection then return end -- Cancelled
        
        local value = type(selection) == "table" and (selection.value or selection) or selection
        
        if value == "__DONE__" then
          if original_on_confirm then
            original_on_confirm(selected_items)
          end
        else
          table.insert(selected_items, value)
          vim.schedule(start_loop)
        end
      end
    })
    
    unl_picker_backend.pick(loop_spec)
  end
  
  start_loop()
end

return M