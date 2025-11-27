-- lua/UNL/backend/picker/provider/telescope.lua

local M = { name = "telescope" }

function M.available()
  return pcall(require, "telescope")
end

function M.run(spec)
  spec = spec or {}
  
  local actions = require("telescope.actions")
  local action_state = require("telescope.actions.state")
  local finders = require("telescope.finders")
  local pickers = require("telescope.pickers")
  local sorters = require("telescope.sorters")
  local conf = require("telescope.config").values
  local log = require("UNL.logging").get(spec.logger_name or "UNL")

  -- DEV ICONS
  local entry_display = require("telescope.pickers.entry_display")
  local devicons_ok, devicons = pcall(require, "nvim-web-devicons")
  local use_devicons = spec.devicons_enabled and devicons_ok

  local displayer, make_display
  if use_devicons then
    displayer = entry_display.create({
      separator = " ",
      items = { { width = 2 }, { remaining = true } },
    })
    make_display = function(entry)
      return displayer({
        { entry.icon, entry.icon_hl },
        entry.display_text,
      })
    end
  end

  local finder
  if spec.items then
    finder = finders.new_table({
      results = spec.items,
      entry_maker = function(entry)
        local value, display, filename, lnum, col
        
        if type(entry) == 'table' then
          value = entry.value or entry
          display = entry.display or entry.label or entry.name or tostring(value)
          filename = entry.filename or (type(value) == 'table' and value.filename)
          lnum = entry.lnum or entry.line or entry.row
          col = entry.col
        else
          value = entry
          display = tostring(entry)
          filename = tostring(entry)
        end
        
        if type(value) == 'table' and string.match(display, "^table: 0x") then
          display = value.display or value.label or value.name or display
        end

        local result = {
          value = value,
          display = display,
          ordinal = display,
          filename = filename,
          lnum = lnum and tonumber(lnum),
          col = col and tonumber(col),
        }
        
        if use_devicons and filename and type(filename) == 'string' then
          local extension = vim.fn.fnamemodify(filename, ":e")
          local icon, icon_hl = devicons.get_icon(filename, extension)
          result.display = make_display
          result.icon = icon or ""
          result.icon_hl = icon_hl or "Normal"
          result.display_text = display
        end

        return result
      end,
    })
  else
    log.warn("Telescope provider (static): No 'items' provided.")
    return
  end

  local picker_opts = {
    prompt_title = spec.title or "Select Item",
    file_ignore_patterns = {},
    find_files_ignore_patterns = {},
    hidden = true ,
    finder = finder,
    sorter = sorters.get_generic_fuzzy_sorter({}),
    cwd = spec.cwd or vim.loop.cwd(),
    attach_mappings = function(prompt_bufnr, map)
      actions.select_default:replace(function()
        local picker = action_state.get_current_picker(prompt_bufnr)
        actions.close(prompt_bufnr)
        
        local get_value = function(entry) return entry and entry.value or nil end

        if spec.multi_select then
          local results = {}
          for _, entry in ipairs(picker:get_multi_selection()) do
            table.insert(results, get_value(entry))
          end
          if #results == 0 then
            local single_entry = action_state.get_selected_entry()
            if single_entry then table.insert(results, get_value(single_entry)) end
          end
          if spec.on_submit then vim.schedule(function() spec.on_submit(results) end) end
        else
          local selection = action_state.get_selected_entry()
          if spec.on_submit then vim.schedule(function() spec.on_submit(get_value(selection)) end) end
        end
      end)
      return true
    end,
  }

  if use_devicons then
    picker_opts.entry_display = make_display
  end

  if spec.preview_enabled ~= false then 
    -- ★★★ 修正: 副作用を防ぐための自動判定ロジック ★★★
    local use_grep_previewer = false
    
    -- 1. specで明示的に指定されている場合
    if spec.preview_mode == "grep" then
        use_grep_previewer = true
    -- 2. 自動判定: アイテムリストの最初の要素が行番号情報を持っているかチェック
    elseif spec.items and #spec.items > 0 then
        local first = spec.items[1]
        if type(first) == "table" and (first.lnum or first.line or first.row) then
            use_grep_previewer = true
        end
    end

    if use_grep_previewer then
        -- 行番号がある場合 -> grep_previewer (定義ジャンプ等に最適)
        picker_opts.previewer = conf.grep_previewer({}) 
    else
        -- 行番号がない場合 -> file_previewer (通常のファイル閲覧に最適)
        picker_opts.previewer = conf.file_previewer({})
    end
  end
  
  pickers.new(picker_opts):find()
end

return M
