-- lua/UNL/backend/dynamic_picker/provider/telescope.lua (nilガードを追加)

local M = { name = "telescope" }

function M.available()
  return pcall(require, "telescope.pickers")
end

function M.run(spec)
  spec = spec or {}
  assert(spec.command, "Telescope dynamic_picker requires 'spec.command'")

  local pickers = require("telescope.pickers")
  local finders = require("telescope.finders")
  local actions = require("telescope.actions")
  local action_state = require("telescope.actions.state")
  local conf = require("telescope.config").values
  local previewers = require("telescope.previewers")
  local devicons_ok, devicons = pcall(require, "nvim-web-devicons")

  local cmd_table = { spec.command }
  if spec.args and type(spec.args) == "table" then
    vim.list_extend(cmd_table, spec.args)
  end

  local picker_opts = {
    prompt_title = spec.title or "Dynamic Items",
    finder = finders.new_oneshot_job(cmd_table, {
      cwd = spec.cwd,
      entry_maker = function(line)
        local parts = vim.split(line, "\t", { plain = true, trimempty = true })
        local display_text = parts[1]
        local file_path = parts[2]

        if not (display_text and file_path) then
          return { display = line, value = line, ordinal = line }
        end
        
        local icon_str = ""
        if devicons_ok and spec.devicons_enabled ~= false then
          local extension = vim.fn.fnamemodify(file_path, ":e")
          -- ▼▼▼【変更点】devicons.get_iconがnilを返しても "or ''" で安全に処理 ▼▼▼
          icon_str = (devicons.get_icon(file_path, extension) or "") .. " "
        end

        return {
          display = icon_str .. display_text,
          value = file_path,
          ordinal = display_text,
          filename = file_path,
        }
      end,
    }),
    sorter = conf.generic_sorter({}),
    attach_mappings = function(prompt_bufnr, map)
      actions.select_default:replace(function()
        actions.close(prompt_bufnr)
        local selection = action_state.get_selected_entry()
        if spec.on_submit then
          vim.schedule(function()
            spec.on_submit(selection and selection.value or nil)
          end)
        end
      end)
      return true
    end,
  }

  if spec.preview_enabled ~= false then
    picker_opts.previewer = conf.file_previewer({})
  end

  pickers.new(picker_opts):find()
end

return M
