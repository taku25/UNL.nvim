-- lua/UNL/backend/picker/provider/telescope.lua (プレビュー機能付き)

local M = { name = "telescope" }

function M.available()
  return pcall(require, "telescope")
end

function M.run(spec)
  local actions = require("telescope.actions")
  local action_state = require("telescope.actions.state")
  local previewers = require("telescope.previewers")
  local kind = spec.kind
  
  local picker_opts = {
    prompt_title = spec.title or "Select Item",
    finder = require("telescope.finders").new_table({
      results = spec.items,
      entry_maker = function(entry)
        local filename_for_preview = (type(entry.value) == "table") and entry.value.filename or (type(entry.value) == "string" and entry.value or nil)
        return {
          value = entry.value,
          display = (spec.format and spec.format(entry)) or entry.label,
          ordinal = entry.label,
          filename = filename_for_preview,
        }
      end,
    }),
    sorter = require("telescope.config").values.generic_sorter({}),
    attach_mappings = function(prompt_bufnr, map)
      actions.select_default:replace(function()
        actions.close(prompt_bufnr)
        local selection = action_state.get_selected_entry()
        if selection and spec.on_submit then
          spec.on_submit(selection.value)
        end
      end)
      return true
    end,
  }

  -- ★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★
  -- ★ ここが、全ての要求を満たす最終的なプレビューロジック ★
  -- ★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★
  local enable_preview = true -- デフォルトは有効
  
  if spec.preview_enabled == false then
    -- 呼び出し側が明示的に「無効」を要求した場合
    enable_preview = false
  elseif spec.preview_enabled == true then
    -- 呼び出し側が明示的に「有効」を要求した場合
    enable_preview = true
  else
    -- preview_enabled の指定がない場合、kind に基づいて賢く判断する
    if kind:match("project") and not kind:match("file") then
      -- "project_cd", "project_delete" のような、ファイルではない場合は
      -- デフォルトでプレビューを無効にする（安全策）
      enable_preview = false
    end
  end

  if enable_preview then
    picker_opts.previewer = previewers.vim_buffer_cat.new({
      title = "Preview",
    })
  end
  -- ★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★

  require("telescope.pickers").new(picker_opts):find()
end

return M
