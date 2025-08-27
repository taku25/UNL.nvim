-- lua/UNL/backend/picker/provider/native.lua

local M = { name = "native" }
function M.available() return true end

local function run_vim_ui_select(spec)
 local choices = {}
 local display_to_value = {}

 for _, item in ipairs(spec.items or {}) do
   local processed
   if spec.entry_maker then
     processed = spec.entry_maker(item)
   else
     processed = { display = item.label or tostring(item), value = item.value or item }
   end
   table.insert(choices, processed.display)
   display_to_value[processed.display] = processed.value
 end

 vim.ui.select(choices, { prompt = spec.title or "Select:" }, function(choice)
   if not choice then
     if spec.on_cancel then spec.on_cancel() end
     return
   end
   if spec.on_submit then
     spec.on_submit(display_to_value[choice])
   end
 end)
end

local function run_quickfix(spec)
  local qf_items = {}
 for _, item in ipairs(spec.items or {}) do
   -- entry_maker が必須。なければ何もしないのが安全。
   if spec.entry_maker then
     local processed = spec.entry_maker(item)
     if processed.filename then
       table.insert(qf_items, {
         filename = processed.filename,
         lnum = processed.lnum or 1,
         col = processed.col or 1,
         text = processed.text or processed.display,
       })
     end
   end
 end

  vim.fn.setqflist({}, " ", { title = spec.title or "File Locations", items = qf_items, })
  vim.api.nvim_command("copen")
end

function M.run(spec)
  if spec.kind == "file_location" then run_quickfix(spec) else run_vim_ui_select(spec) end
end

return M
