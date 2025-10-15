local M = { name = "snacks" }

function M.available()
  return pcall(require, "snacks")
end

function M.run(spec)
  spec = spec or {}
  local Snacks = require("snacks")

  local snacks_opts = {
    title = spec.title or "Find Results",
    cwd = spec.cwd,
  }

  -- 1. Finderを定義: 各行を `{ file = ... }` 形式のテーブルに変換
  if spec.exec_cmd and type(spec.exec_cmd) == "table" then
    snacks_opts.finder = function()
      local results = vim.fn.systemlist(spec.exec_cmd)
      --【修正点】
      -- snacksの"file"フォーマッターが認識できるように、キーを'text'から'file'に変更
      return vim.tbl_map(function(line)
        return { file = line, text = line }
      end, results)
    end
  else
    require("UNL.logging").get(spec.logger_name or "UNL"):error("snacks.nvim find_picker: spec.exec_cmd is required.")
    return
  end

  -- 2. プレビューとフォーマットを設定
  if spec.preview_enabled ~= false then
    snacks_opts.format = "file"
    snacks_opts.preview = "file"
  else
    snacks_opts.layout = { hidden = { "preview" } }
  end

  -- 3. 'confirm' アクションを上書き
  snacks_opts.actions = {}
  if spec.on_submit then
    snacks_opts.actions.confirm = function(picker, item)
      if item then
        Snacks.picker.actions.close(picker)
        --【修正点】
        -- finderで設定した'item.file'から値を取得
        vim.schedule(function() spec.on_submit(item.file) end)
      else
        Snacks.picker.actions.close(picker)
      end
    end
  end

  -- 4. ESCキーの動作を設定
  snacks_opts.win = { input = { keys = {} }, list = { keys = {} } }
  local esc_action = function(picker)
    if spec.on_cancel then vim.schedule(spec.on_cancel) end
    Snacks.picker.actions.close(picker)
  end
  snacks_opts.win.input.keys["<Esc>"] = esc_action
  snacks_opts.win.list.keys["<Esc>"] = esc_action

  -- 5. ピッカーを実行
  Snacks.picker.pick(snacks_opts)
end

return M
