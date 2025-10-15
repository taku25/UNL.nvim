local M = { name = "snacks" }

function M.available()
  return pcall(require, "snacks")
end

function M.run(spec)
  spec = spec or {}
  local Snacks = require("snacks")

  local grep_opts = {
    title = spec.title or "Live Grep",
    dirs = spec.search_paths,
    exclude = spec.exclude_directories,
  }

  if spec.include_extensions and #spec.include_extensions > 0 then
    grep_opts.glob = vim.tbl_map(function(ext) return "*." .. ext end, spec.include_extensions)
  end

  grep_opts.actions = {}
  if spec.on_submit then
    grep_opts.actions.confirm = function(picker, item)
      --【修正点】
      -- 'item.loc'ではなく、実際のデータ構造である'item.file'と'item.pos'をチェックします。
      if item and item.file and item.pos then
        Snacks.picker.actions.close(picker)
        vim.schedule(function()
          spec.on_submit({
            filename = item.file, -- 'item.file' を使用
            lnum = item.pos[1],     -- 'item.pos'の最初の要素を行番号として使用
            col = item.pos[2],      -- 'item.pos'の2番目の要素を列番号として使用
          })
        end)
      else
        Snacks.picker.actions.close(picker)
      end
    end
  end

  grep_opts.win = { input = { keys = {} }, list = { keys = {} } }
  local esc_action = function(picker)
    if spec.on_cancel then vim.schedule(spec.on_cancel) end
    Snacks.picker.actions.close(picker)
  end
  grep_opts.win.input.keys["<Esc>"] = esc_action
  grep_opts.win.list.keys["<Esc>"] = esc_action

  Snacks.picker.grep(grep_opts)
end

return M
