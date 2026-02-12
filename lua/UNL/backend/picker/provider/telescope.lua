-- lua/UNL/backend/picker/provider/telescope.lua
-- Unified Telescope provider for UNL.nvim

local M = { name = "telescope" }

function M.available()
  return pcall(require, "telescope")
end

function M.run(spec)
  spec = spec or {}
  local source = spec.source or { type = "static", items = spec.items }
  
  if source.type == "static" then
    return M.run_static(spec, source)
  elseif source.type == "grep" then
    return M.run_grep(spec, source)
  elseif source.type == "callback" then
    return M.run_callback(spec, source)
  elseif source.type == "job" then
    return M.run_job(spec, source)
  end
end

-- HELPER: Create entry maker with devicons
local function create_entry_maker(spec, use_devicons, devicons, make_display)
  return function(entry)
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
      result.icon = icon or (type(entry) == "table" and entry.icon) or ""
      result.icon_hl = icon_hl or "Normal"
      result.display_text = display
      -- ★ 修正: make_display が存在する場合のみ上書きする
      if make_display then
        result.display = make_display
      end
    else
      result.icon = (type(entry) == "table" and entry.icon) or ""
      result.icon_hl = "Normal"
    end
    return result
  end
end

function M.run_static(spec, source)
  local actions = require("telescope.actions")
  local action_state = require("telescope.actions.state")
  local finders = require("telescope.finders")
  local pickers = require("telescope.pickers")
  local sorters = require("telescope.sorters")
  local conf = require("telescope.config").values
  local entry_display = require("telescope.pickers.entry_display")
  local devicons_ok, devicons = pcall(require, "nvim-web-devicons")
  local use_devicons = spec.devicons_enabled ~= false and devicons_ok

  local displayer, make_display
  if use_devicons then
    displayer = entry_display.create({ separator = " ", items = { { width = 2 }, { remaining = true } } })
    make_display = function(entry) return displayer({ { entry.icon, entry.icon_hl }, entry.display_text }) end
  end

  local finder = finders.new_table({
    results = source.items or {},
    entry_maker = create_entry_maker(spec, use_devicons, devicons, make_display)
  })

  local picker_opts = {
    prompt_title = spec.title or "Select",
    finder = finder,
    sorter = sorters.get_generic_fuzzy_sorter({}),
    attach_mappings = function(prompt_bufnr)
      actions.select_default:replace(function()
        local picker = action_state.get_current_picker(prompt_bufnr)
        actions.close(prompt_bufnr)
        local get_value = function(entry) return entry and entry.value or nil end
        local is_multi = (spec.multiselect == "native" or spec.multiselect == true)
        if is_multi then
          local results = {}
          for _, entry in ipairs(picker:get_multi_selection()) do table.insert(results, get_value(entry)) end
          if #results == 0 then
            local e = action_state.get_selected_entry()
            if e then table.insert(results, get_value(e)) end
          end
          if spec.on_confirm then vim.schedule(function() spec.on_confirm(results) end) end
        else
          local e = action_state.get_selected_entry()
          if spec.on_confirm then vim.schedule(function() spec.on_confirm(get_value(e)) end) end
        end
      end)
      return true
    end,
  }

  if spec.preview_enabled ~= false then
    local use_grep_previewer = false
    if spec.preview_mode == "grep" then use_grep_previewer = true
    elseif source.items and #source.items > 0 then
      local first = source.items[1]
      if type(first) == "table" and (first.lnum or first.line or first.row) then use_grep_previewer = true end
    end
    picker_opts.previewer = use_grep_previewer and conf.grep_previewer({}) or conf.file_previewer({})
  end

  pickers.new(picker_opts):find()
end

function M.run_grep(spec, source)
  local builtin = require('telescope.builtin')
  local actions = require("telescope.actions")
  local action_state = require("telescope.actions.state")
  local make_entry = require("telescope.make_entry")
  local entry_display = require("telescope.pickers.entry_display")
  local devicons_ok, devicons = pcall(require, "nvim-web-devicons")
  local use_devicons = spec.devicons_enabled ~= false and devicons_ok

  local displayer, make_display
  if use_devicons then
    displayer = entry_display.create({ separator = " ", items = { { width = 2 }, { remaining = true } } })
    make_display = function(entry) return displayer({ { entry.icon, entry.icon_hl }, entry.display_text }) end
  end

  local args = {}
  for _, dir in ipairs(source.exclude_directories or {}) do
    table.insert(args, "--glob"); table.insert(args, "!" .. dir)
  end
  for _, ext in ipairs(source.include_extensions or {}) do
    table.insert(args, "-g"); table.insert(args, "*." .. ext)
  end

  local grep_opts = {
    prompt_title = spec.title or "Live Grep",
    search_dirs = source.search_paths,
    additional_args = args,
    attach_mappings = function(bufnr)
      actions.select_default:replace(function()
        actions.close(bufnr)
        local e = action_state.get_selected_entry()
        if e and spec.on_confirm then
          vim.schedule(function() spec.on_confirm({ filename = e.filename, lnum = e.lnum, col = e.col }) end)
        end
      end)
      return true
    end,
  }

  if spec.transform_display or use_devicons then
    local default_maker = make_entry.gen_from_vimgrep(spec)
    grep_opts.entry_maker = function(line)
      local entry = default_maker(line)
      if not entry then return nil end
      local disp_path = (spec.transform_display and spec.transform_display(entry.filename)) or entry.filename
      local disp_text = string.format("%s:%s:%s:%s", disp_path, entry.lnum, entry.col, entry.text)
      if use_devicons then
        local icon, hl = devicons.get_icon(entry.filename, vim.fn.fnamemodify(entry.filename, ":e"))
        entry.display, entry.icon, entry.icon_hl, entry.display_text = make_display, icon or "", hl or "Normal", disp_text
      else
        entry.display = disp_text
      end
      return entry
    end
  end

  builtin.live_grep(grep_opts)
end

function M.run_callback(spec, source)
  local actions = require("telescope.actions")
  local action_state = require("telescope.actions.state")
  local finders = require("telescope.finders")
  local pickers = require("telescope.pickers")
  local conf = require("telescope.config").values
  local entry_display = require("telescope.pickers.entry_display")
  local devicons_ok, devicons = pcall(require, "nvim-web-devicons")
  local use_devicons = spec.devicons_enabled ~= false and devicons_ok

  local results = {}
  
  -- ★ 修正: callback 用にも displayer を準備する
  local displayer, make_display
  if use_devicons then
    displayer = entry_display.create({ separator = " ", items = { { width = 2 }, { remaining = true } } })
    make_display = function(entry) return displayer({ { entry.icon, entry.icon_hl }, entry.display_text }) end
  end

  local entry_maker = create_entry_maker(spec, use_devicons, devicons, make_display)

  local finder = setmetatable({ results = results, close = function() end }, {
    __call = function(_, _, cb, cb_complete)
      for _, item in ipairs(results) do
        local e = entry_maker(item)
        if e then cb(e) end
      end
      if cb_complete then cb_complete() end
    end
  })

  local picker = pickers.new({
    prompt_title = spec.title or "Dynamic Picker",
    finder = finder,
    sorter = conf.generic_sorter({}),
    sorting_strategy = "ascending",
    attach_mappings = function(bufnr)
      actions.select_default:replace(function()
        local selection = action_state.get_selected_entry()
        actions.close(bufnr)
        if selection and spec.on_confirm then
          vim.schedule(function() spec.on_confirm(selection.value) end)
        end
      end)
      return true
    end,
  })
  picker.tiebreak = function() return false end
  picker:find()

  local push = function(items)
    if not items then return end
    local to_add = (type(items) == "table" and items[1] ~= nil) and items or {items}
    for _, it in ipairs(to_add) do table.insert(results, it) end
    
    -- UIの更新を依頼
    vim.schedule(function()
      if picker.prompt_bufnr and vim.api.nvim_buf_is_valid(picker.prompt_bufnr) then
        -- Telescope の内部状態をリフレッシュする
        if picker._on_lines then
          picker._on_lines()
        end
      end
    end)
  end

  if source.fn then source.fn(push) end
end

function M.run_job(spec, source)
  local finders = require("telescope.finders")
  local pickers = require("telescope.pickers")
  local actions = require("telescope.actions")
  local action_state = require("telescope.actions.state")
  local conf = require("telescope.config").values
  local log = require("UNL.logging").get(spec.logger_name or "UNL")

  if not source.command then
    log.error("Telescope job source: 'command' (table) is required.")
    return
  end

  local finder = finders.new_oneshot_job(source.command, {
    entry_maker = function(line)
      return { value = line, display = line, ordinal = line, filename = line }
    end,
  })

  local picker_opts = {
    prompt_title = spec.title or "Find Results",
    finder = finder,
    sorter = conf.generic_sorter({}),
    attach_mappings = function(bufnr)
      actions.select_default:replace(function()
        actions.close(bufnr)
        local selection = action_state.get_selected_entry()
        if selection and spec.on_confirm then
          vim.schedule(function() spec.on_confirm(selection.value) end)
        end
      end)
      return true
    end,
  }

  if spec.preview_enabled then
    picker_opts.previewer = conf.file_previewer({})
  end

  pickers.new(picker_opts):find()
end

return M
