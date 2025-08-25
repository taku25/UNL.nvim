-- lua/UNL/backend/picker/provider/native.lua
-- (内容は前回の提案と同じ)
local M = { name = "native" }
function M.available() return true end

local function run_vim_ui_select(spec)
  local choices = {}; for _, item in ipairs(spec.items) do table.insert(choices, (spec.format and spec.format(item)) or item.label) end
  vim.ui.select(choices, { prompt = spec.title or "Select:" }, function(choice)
    if not choice then if spec.on_cancel then spec.on_cancel() end; return end
    for _, item in ipairs(spec.items) do if ((spec.format and spec.format(item)) or item.label) == choice then if spec.on_submit then spec.on_submit(item.value) end; return end end
  end)
end
local function run_quickfix(spec)
  local qf_items = {}
  for _, item in ipairs(spec.items) do
    -- ★ ここが正しく動くようになる ★
    -- item.value は { filename = ..., text = ... } なので、
    -- item.value.filename は、正しいファイルパスを返す。
    table.insert(qf_items, {
      filename = item.value.filename,
      lnum = item.value.lnum or 1,
      col = item.value.col or 1,
      text = item.value.text or item.label,
    })
  end
  vim.fn.setqflist({}, " ", { title = spec.title or "File Locations", items = qf_items, })
  vim.api.nvim_command("copen")
end

function M.run(spec)
  if spec.kind == "file_location" then run_quickfix(spec) else run_vim_ui_select(spec) end
end
return M
