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
      item.value = item.value or item
      item.display = item.display or item.label or item.name or tostring(item.value)
      item.filename = item.filename or (type(item.value) == "table" and item.value.filename)
    end

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

    if type(item.value) == "table" and string.match(item.display, "^table: 0x") then
      item.display = item.value.display or item.value.label or item.value.name or item.display
    end

    -- text がない場合、display や value から生成 (Matcherエラー回避)
    if not item.text then
      item.text = item.display or (type(item.value) == "string" and item.value) or item.file or ""
    end
  end

  local snacks_opts = {
    title = spec.title or "Select Item",
    cwd = spec.cwd,
    items = items,
  }

  snacks_opts.format = function(item)
    local value, display, filename, lnum, col

    if type(item) == "table" then
      value = item.value
      display = item.display
      filename = item.filename
    else
      value = item
      display = tostring(item)
      filename = tostring(item)
    end

    local processed = {
      display = display,
      filename = filename,
      value = value,
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
        vim.schedule(function()
          spec.on_submit(item.unl_value)
        end)
      end
    end
    snacks_opts.confirm = "unl_submit"
  end

  if spec.on_cancel then
    snacks_opts.actions.cancel = function(picker)
      vim.schedule(spec.on_cancel)
      picker:norm(function()
        picker.main = picker:filter().current_win
        picker:close()
      end)
    end
  end

  Snacks.picker.pick(snacks_opts)
end

return M
