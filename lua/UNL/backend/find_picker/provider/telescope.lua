-- lua/UNL/backend/find_picker/provider/telescope.lua

local M = { name = "telescope" }

function M.available()
  return pcall(require, "telescope")
end

function M.run(spec)
  spec = spec or {}
  local log = require("UNL.logging").get(spec.logger_name or "UNL")

  if not (spec.exec_cmd and type(spec.exec_cmd) == "table") then
    log.error("Telescope find_picker: exec_cmd is required.")
    return
  end

  local finders = require("telescope.finders")
  local pickers = require("telescope.pickers")
  local actions = require("telescope.actions")
  local action_state = require("telescope.actions.state")
  local previewers = require("telescope.previewers") -- (追加) プレビューワーを読み込む

  local finder = finders.new_oneshot_job(spec.exec_cmd, {
    entry_maker = function(line)
      return {
        value = line,
        display = line,
        ordinal = line,
        -- (追加) プレビューのために、結果の行自体をファイルパスとして渡す
        filename = line,
      }
    end,
  })

  -- (変更) pickerのオプションを一度テーブルに格納する
  local picker_opts = {
    prompt_title = spec.title or "Find Results",
    finder = finder,
    file_ignore_patterns = spec.file_ignore_patterns or {},
    sorter = require("telescope.config").values.generic_sorter(),
    attach_mappings = function(bufnr, map)
      actions.select_default:replace(function()
        actions.close(bufnr)
        local selection = action_state.get_selected_entry()
        if selection and spec.on_submit then
          pcall(spec.on_submit, selection.value)
        end
      end)
      return true
    end,
  }

  -- (追加) もし spec.preview_enabled が true なら、プレビューワーを追加する
  if spec.preview_enabled then
    -- ファイル内容を表示する、標準的なプレビューワーを使用
    picker_opts.previewer = previewers.vim_buffer_cat.new({
      title = "Preview",
    })
  end

  pickers.new(picker_opts):find()
end

return M
