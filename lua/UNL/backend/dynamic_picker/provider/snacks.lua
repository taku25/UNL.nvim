local M = { name = "snacks" }

function M.available()
  return pcall(require, "snacks")
end

function M.run(spec)
  spec = spec or {}
  local Snacks = require("snacks")
  local devicons_ok, devicons = pcall(require, "nvim-web-devicons")

  local snacks_opts = {
    title = spec.title or "Dynamic Items",
    cwd = spec.cwd,
  }

  -- 1. Finderを定義: コマンドを実行し、タブ区切りの出力をパースする
  if spec.command then
    snacks_opts.finder = function()
      local cmd_table = { spec.command }
      if spec.args and type(spec.args) == "table" then
        vim.list_extend(cmd_table, spec.args)
      end
      local results = vim.fn.systemlist(cmd_table)
      
      return vim.tbl_map(function(line)
        local parts = vim.split(line, "\t", { plain = true, trimempty = true })
        local display_text, file_path = parts[1], parts[2]
        -- 【重要】
        -- text: 絞り込み検索用
        -- file: プレビューと選択アクション用
        -- display: フォーマッターでの表示用
        return { display = display_text, file = file_path, text = display_text }
      end, results)
    end
  else
    require("UNL.logging").get(spec.logger_name or "UNL"):error("snacks.nvim dynamic_picker: spec.command is required.")
    return
  end

  -- 2. フォーマッターを定義: deviconと表示名を組み合わせる
  snacks_opts.format = function(item)
    local highlights = {}
    if spec.devicons_enabled and devicons_ok and item.file then
      local icon, icon_hl = devicons.get_icon(item.file, vim.fn.fnamemodify(item.file, ":e"))
      if icon then
        table.insert(highlights, { icon .. " ", icon_hl or "Normal" })
      end
    end
    table.insert(highlights, { item.display or item.text or "" })
    return highlights
  end

  -- 3. プレビューを設定
  if spec.preview_enabled ~= false then
    snacks_opts.preview = "file"
  else
    snacks_opts.layout = { hidden = { "preview" } }
  end

  -- 4. 'confirm' アクションを上書きしてon_submitを呼び出す
  snacks_opts.actions = {}
  if spec.on_submit then
    snacks_opts.actions.confirm = function(picker, item)
      if item and item.file then
        Snacks.picker.actions.close(picker)
        vim.schedule(function() spec.on_submit(item.file) end)
      else
        Snacks.picker.actions.close(picker)
      end
    end
  end

  -- 5. ESCキーの動作を設定
  snacks_opts.win = { input = { keys = {} }, list = { keys = {} } }
  local esc_action = function(picker)
    if spec.on_cancel then vim.schedule(spec.on_cancel) end
    Snacks.picker.actions.close(picker)
  end
  snacks_opts.win.input.keys["<Esc>"] = esc_action
  snacks_opts.win.list.keys["<Esc>"] = esc_action

  -- 6. ピッカーを実行
  Snacks.picker.pick(snacks_opts)
end

return M
