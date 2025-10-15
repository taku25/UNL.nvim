local M = { name = "snacks" }

function M.available()
  return pcall(require, "snacks")
end

function M.run(spec)
  spec = spec or {}
  local Snacks = require("snacks")
  local devicons_ok, devicons = pcall(require, "nvim-web-devicons")

  local snacks_opts = {
    title = spec.title or "Select Item",
    cwd = spec.cwd,
    items = spec.items or {},
  }

  -- 1. フォーマッターを設定 (Deviconsとプレビュー対応)
  snacks_opts.format = function(item)
    local processed = (spec.entry_maker and spec.entry_maker(item)) or {
      display = tostring(item.display or item.value or item.text or item),
      filename = item.filename,
      value = item.value or item,
    }
    if processed.filename then
      item.file = processed.filename
    end
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

  -- 2. プレビューを設定
  if spec.preview_enabled ~= false then
    snacks_opts.preview = "file"
  else
    snacks_opts.layout = { hidden = { "preview" } }
  end

  -- 3. アクションを設定
  snacks_opts.actions = {}

  if spec.on_submit then
    --【修正点】
    -- アクション関数の第一引数 'picker' を受け取る
    snacks_opts.actions.unl_submit = function(picker, item)
      if item then
        -- 受け取った 'picker' をcloseアクションに渡す
        Snacks.picker.actions.close(picker)
        vim.schedule(function() spec.on_submit(item.unl_value) end)
      end
    end
    snacks_opts.confirm = "unl_submit"
  end

  -- 4. ESCキーのアクションを設定
  snacks_opts.win = { input = { keys = {} }, list = { keys = {} } }
  local esc_action = function(picker)
    if spec.on_cancel then
      vim.schedule(spec.on_cancel)
    end
    -- snacks標準のクローズアクションを実行
    Snacks.picker.actions.close(picker)
  end
  snacks_opts.win.input.keys["<Esc>"] = esc_action
  snacks_opts.win.list.keys["<Esc>"] = esc_action

  -- 5. ピッカーを実行
  Snacks.picker.pick(snacks_opts)
end

return M
