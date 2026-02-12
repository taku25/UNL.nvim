-- lua/UNL/backend/picker/provider/native.lua
-- Unified Native picker provider for UNL.nvim (vim.ui.select / quickfix)

local M = { name = "native" }
function M.available() return true end

function M.run(spec)
  spec = spec or {}
  local source = spec.source or { type = "static", items = spec.items }
  
  if source.type == "static" then
    if spec.kind == "file_location" then
      return M.run_quickfix(spec, source)
    else
      return M.run_vim_ui_select(spec, source)
    end
  else
    -- Grep and Callback are not well supported by native vim.ui.select
    -- We'll just log a warning and do nothing or fallback
    local log = require("UNL.logging").get(spec.logger_name or "UNL")
    log.warn("Native provider does not support " .. source.type .. " source type.")
  end
end

function M.run_vim_ui_select(spec, source)
  local choices = {}
  local lookup = {}

  for _, item in ipairs(source.items or {}) do
    local display = type(item) == "table" and (item.display or item.label or item.name or tostring(item.value or item)) or tostring(item)
    local value = type(item) == "table" and (item.value or item) or item
    table.insert(choices, display)
    lookup[display] = value
  end

  vim.ui.select(choices, { prompt = spec.title or "Select:" }, function(choice)
    if not choice then
      if spec.on_cancel then spec.on_cancel() end
      return
    end
    if spec.on_confirm then spec.on_confirm(lookup[choice]) end
  end)
end

function M.run_quickfix(spec, source)
  local qf_items = {}
  for _, item in ipairs(source.items or {}) do
    if type(item) == "table" and item.filename then
      table.insert(qf_items, {
        filename = item.filename,
        lnum = item.lnum or item.line or item.row or 1,
        col = item.col or 1,
        text = item.display or item.label or item.name or item.filename,
      })
    end
  end
  vim.fn.setqflist({}, " ", { title = spec.title or "File Locations", items = qf_items })
  vim.api.nvim_command("copen")
end

return M