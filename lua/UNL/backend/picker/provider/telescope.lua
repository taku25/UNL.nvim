-- lua/UNL/backend/picker/provider/telescope.lua

local M = { name = "telescope" }

function M.available()
  return pcall(require, "telescope")
end

function M.run(spec)
  local actions = require("telescope.actions")
  local action_state = require("telescope.actions.state")
  local previewers = require("telescope.previewers")
  local finders = require("telescope.finders")
  local pickers = require("telescope.pickers")
  local sorters = require("telescope.sorters")
  local log = require("UNL.logging").get("UNL")

  spec = spec or {}
  local kind = spec.kind
  local finder

  if spec.items then
    -- Case 1: items (テーブル) が渡された場合
    finder = finders.new_table({
      results = spec.items,
      entry_maker = function(entry)
        local filename_for_preview = (type(entry.value) == "table") and entry.value.filename or (type(entry.value) == "string" and entry.value or nil)
        return {
          value = entry.value,
          display = (spec.format and spec.format(entry)) or entry.label or tostring(entry),
          ordinal = entry.label or tostring(entry),
          filename = filename_for_preview,
        }
      end,
    })
  elseif spec.exec_cmd then
    -- Case 2: exec_cmd (直接コマンド) が渡された場合
    local cmd = type(spec.exec_cmd) == "string" and vim.split(spec.exec_cmd, " ") or spec.exec_cmd
    finder = finders.new_oneshot_job(cmd, {
      entry_maker = function(line)
        return { value = line, display = line, ordinal = line, filename = line }
      end,
    })
  else
    log.warn("Telescope provider: No items or exec_cmd provided.")
    return
  end

  local picker_opts = {
    prompt_title = spec.title or "Select Item",
    finder = finder,
    sorter = sorters.get_generic_fuzzy_sorter({}),
    cwd = spec.cwd or vim.loop.cwd(),
    attach_mappings = function(prompt_bufnr, map)
      actions.select_default:replace(function()
        local selection = action_state.get_selected_entry()
        actions.close(prompt_bufnr)
        if spec.on_submit then
          vim.schedule(function()
            spec.on_submit(selection and selection.value or nil)
          end)
        end
      end)
      return true
    end,
  }

  -- プレビューロジック
  local enable_preview = true
  if spec.preview_enabled == false then
    enable_preview = false
  elseif spec.preview_enabled == true then
    enable_preview = true
  else
    if kind and kind:match("project") and not kind:match("file") then
      enable_preview = false
    end
  end

  if enable_preview then
    picker_opts.previewer = previewers.vim_buffer_cat.new({ title = "Preview" })
  end

  pickers.new(picker_opts):find()
end

return M
