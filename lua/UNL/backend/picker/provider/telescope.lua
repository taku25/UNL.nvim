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
  local log = require("UNL.logging").get(spec.logger_name or "UNL")

  spec = spec or {}
  local display_to_value = {}

  if spec.items then
    finder = finders.new_table({
      results = spec.items,
      -- ★★★ ここが、文字列とテーブルの両方を完璧に扱う、最終的なentry_makerです ★★★
      entry_maker = function(entry)
        local value, display
        if type(entry) == 'table' then
          value = entry.value or entry
          -- テーブルの場合は、一般的なキーを順番に試す
          display = entry.display or entry.label or entry.name or tostring(value)
        else
          -- 文字列や数値の場合は、そのまま使う
          value = entry
          display = tostring(entry)
        end
        
        -- もしdisplayがまだテーブルのメモリアドレスだったら、最後の悪あがきをする
        if type(value) == 'table' and string.match(display, "^table: 0x") then
          display = value.display or value.label or value.name or display
        end

        display_to_value[display] = value
        return { value = value, display = display, ordinal = display, filename = (type(value) == "table" and value.filename) or (type(value) == "string" and value or nil) }
      end,
    })
  elseif spec.exec_cmd then
    finder = finders.new_oneshot_job(vim.split(spec.exec_cmd, " "), {
      entry_maker = function(line) return { value = line, display = line, ordinal = line, filename = line } end,
    })
  else
    log.warn("Telescope provider: No items or exec_cmd provided."); return
  end

  local picker_opts = {
    prompt_title = spec.title or "Select Item",
    finder = finder,
    sorter = sorters.get_generic_fuzzy_sorter({}),
    cwd = spec.cwd or vim.loop.cwd(),
    attach_mappings = function(prompt_bufnr, map)
      actions.select_default:replace(function()
        local picker = action_state.get_current_picker(prompt_bufnr)
        actions.close(prompt_bufnr)
        local function get_value_from_entry(entry)
          if entry and entry.value then return entry.value end
          if entry and entry.display and display_to_value[entry.display] then return display_to_value[entry.display] end
          if entry and entry.display then return entry.display end
          return nil
        end
        if spec.multi_select then
          local selections = picker:get_multi_selection()
          local single_selection = action_state.get_selected_entry()
          if #selections > 0 then
            if spec.on_submit then
              local results = {}; for _, entry in ipairs(selections) do table.insert(results, get_value_from_entry(entry)) end
              vim.schedule(function() spec.on_submit(results) end)
            end
          elseif single_selection and spec.on_submit then
            vim.schedule(function() spec.on_submit({ get_value_from_entry(single_selection) }) end)
          elseif spec.on_submit then
            vim.schedule(function() spec.on_submit({}) end)
          end
        else
          local selection = action_state.get_selected_entry()
          if spec.on_submit then vim.schedule(function() spec.on_submit(get_value_from_entry(selection)) end) end
        end
      end)
      return true
    end,
  }

  if spec.preview_enabled ~= false then picker_opts.previewer = previewers.vim_buffer_cat.new({ title = "Preview" }) end
  pickers.new(picker_opts):find()
end

return M
