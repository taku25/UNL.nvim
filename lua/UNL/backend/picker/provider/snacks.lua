-- lua/UNL/backend/picker/provider/snacks.lua
local M = { name = "snacks" }

function M.available()
  return pcall(require, "snacks")
end

function M.run(spec)
  spec = spec or {}
  local Snacks = require("snacks")
  local devicons_ok, devicons = pcall(require, "nvim-web-devicons")

  -- ★★★ 修正: 入力アイテムを前処理して、Snacks形式 (pos) に合わせる ★★★
  local items = spec.items or {}
  for _, item in ipairs(items) do
    if type(item) == "table" then
      -- lnum / line / row があれば pos = {line, col} を作成
      local l = item.lnum or item.line or item.row
      local c = item.col or 0
      if l then
        item.pos = { l, c }
      end
      -- filename があれば file にコピー (Snacksは file を見る場合がある)
      if item.filename and not item.file then
        item.file = item.filename
      end
    end
  end

  local snacks_opts = {
    title = spec.title or "Select Item",
    cwd = spec.cwd,
    items = items,
  }

  snacks_opts.format = function(item)
    local processed = (spec.entry_maker and spec.entry_maker(item)) or {
      display = tostring(item.display or item.value or item.text or item),
      filename = item.filename or item.file,
      value = item.value or item,
    }
    
    local highlights = {}
    if spec.devicons_enabled and devicons_ok and processed.filename then
      local icon, icon_hl = devicons.get_icon(processed.filename, vim.fn.fnamemodify(processed.filename, ":e"))
      if icon then
        table.insert(highlights, { icon .. " ", icon_hl or "Normal" })
      end
    end
    table.insert(highlights, { processed.display })
    item.unl_value = processed.value
    return highlights
  end

  if spec.preview_enabled ~= false then
    snacks_opts.preview = "file"
  else
    snacks_opts.layout = { hidden = { "preview" } }
  end

  snacks_opts.actions = {}

  if spec.on_submit then
    snacks_opts.actions.unl_submit = function(picker, item)
      if item then
        Snacks.picker.actions.close(picker)
        vim.schedule(function() spec.on_submit(item.unl_value) end)
      end
    end
    snacks_opts.confirm = "unl_submit"
  end

  snacks_opts.win = { input = { keys = {} }, list = { keys = {} } }
  local esc_action = function(picker)
    if spec.on_cancel then
      vim.schedule(spec.on_cancel)
    end
    Snacks.picker.actions.close(picker)
  end
  snacks_opts.win.input.keys["<Esc>"] = esc_action
  snacks_opts.win.list.keys["<Esc>"] = esc_action

  Snacks.picker.pick(snacks_opts)
end

return M
