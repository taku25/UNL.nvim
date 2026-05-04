-- lua/UNL/backend/picker/provider/telescope.lua
-- Unified Telescope provider for UNL.nvim

local M = { name = "telescope" }

function M.available()
  return pcall(require, "telescope")
end

local function to_telescope_item(use_devicons, devicons, make_display)
  return function(entry)
    local value, display, filename, lnum, col

    if type(entry) == "table" then
      value = entry.value or entry
      display = entry.display or entry.label or entry.name or tostring(value)
      filename = entry.filename or (type(value) == "table" and value.filename)
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

    if use_devicons and filename and type(filename) == "string" then
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

local behaviour = {
  single = {
    native = function(picker, spec)
      local opts = picker.opts
      local actions = require("telescope.actions")
      local action_state = require("telescope.actions.state")
      opts.attach_mappings = function(prompt_bufnr)
        actions.select_default:replace(function()
          actions.close(prompt_bufnr)
          local e = action_state.get_selected_entry()
          if spec.on_confirm then
            vim.schedule(function()
              spec.on_confirm(opts.handle_item(e))
            end)
          end
        end)
        return true
      end
    end,
  },
  multiselect = {
    native = function(picker, spec)
      local opts = picker.opts
      local actions = require("telescope.actions")
      local action_state = require("telescope.actions.state")
      opts.attach_mappings = function(prompt_bufnr)
        actions.select_default:replace(function()
          local picker = action_state.get_current_picker(prompt_bufnr)
          actions.close(prompt_bufnr)
          local results = {}
          for _, entry in ipairs(picker:get_multi_selection()) do
            table.insert(results, opts.handle_item(entry))
          end
          if #results == 0 then
            local e = action_state.get_selected_entry()
            if e then
              table.insert(results, opts.handle_item(e))
            end
          end
          if spec.on_confirm then
            vim.schedule(function()
              spec.on_confirm(results)
            end)
          end
        end)
        -- local default_check = spec.default_selected and true or false
        -- if default_check then
        -- 	vim.schedule(function()
        -- 		actions.select_all(prompt_bufnr)
        -- 	end)
        -- end
        return true
      end
    end,
    loop = function(picker, spec)
      local opts = picker.opts
      local actions = require("telescope.actions")
      local action_state = require("telescope.actions.state")
      if opts.finder.results then
        local default_check = spec.default_selected and true or false
        for _, item in ipairs(opts.finder.results) do
          if type(item.display) == "function" then
            item.base = item.display_text
            item.checked = default_check
            item.display_text = (item.checked and "󰄲 " or " ") .. item.base
          else
            item.base = item.display
            item.checked = default_check
            item.display = (item.checked and "󰄲 " or " ") .. item.base
          end
        end
        table.insert(opts.finder.results, 1, opts.finder.entry_maker("* Confirm selection"))
      end
      opts.attach_mappings = function(prompt_bufnr)
        actions.select_default:replace(function()
          -- local picker = action_state.get_current_picker(prompt_bufnr)
          local e = action_state.get_selected_entry()
          local picker = action_state.get_current_picker(prompt_bufnr)
          if (type(e.display) == "function" and e.display_text or e.display) == "* Confirm selection" then
            actions.close(prompt_bufnr)
            local results = {}
            for _, entry in ipairs(picker.finder.results) do
              if
                (type(entry.display) == "function" and entry.display_text or entry.display)
                ~= "* Confirm selection"
              then
                if entry.checked then
                  table.insert(results, opts.handle_item(entry))
                end
              end
            end
            if spec.on_confirm then
              vim.schedule(function()
                spec.on_confirm(results)
              end)
            end
            return
          else
            e.checked = not e.checked
            if type(e.display) == "function" then
              e.display_text = (e.checked and "󰄲 " or " ") .. e.base
            else
              e.display = (e.checked and "󰄲 " or " ") .. e.base
            end
            local cur_row = picker:get_selection_row()
            picker:refresh()
            picker:set_selection(cur_row)
          end
        end)
        -- local default_check = spec.default_selected and true or false
        -- if default_check then
        -- 	vim.schedule(function()
        -- 		actions.select_all(prompt_bufnr)
        -- 	end)
        -- end
        return true
      end
    end,
  },
  multiselect_empty = {
    native = function(picker, spec)
      local opts = picker.opts
      local actions = require("telescope.actions")
      local action_state = require("telescope.actions.state")
      opts.attach_mappings = function(prompt_bufnr)
        actions.select_default:replace(function()
          local picker = action_state.get_current_picker(prompt_bufnr)
          actions.close(prompt_bufnr)
          local results = {}
          for _, entry in ipairs(picker:get_multi_selection()) do
            table.insert(results, opts.handle_item(entry))
          end
          if spec.on_confirm then
            vim.schedule(function()
              spec.on_confirm(results)
            end)
          end
        end)
        -- local default_check = spec.default_selected and true or false
        -- if default_check then
        -- 	vim.schedule(function()
        -- 		actions.select_all(prompt_bufnr)
        -- 	end)
        -- end
        return true
      end
    end,
    confirm_item = function(picker, spec)
      local opts = picker.opts
      local actions = require("telescope.actions")
      local action_state = require("telescope.actions.state")
      table.insert(opts.finder.results, 1, opts.finder.entry_maker("* Confirm selection"))
      opts.attach_mappings = function(prompt_bufnr)
        actions.select_default:replace(function()
          local picker = action_state.get_current_picker(prompt_bufnr)
          actions.close(prompt_bufnr)
          local results = {}
          local validated = false
          for _, entry in ipairs(picker:get_multi_selection()) do
            local val = opts.handle_item(entry)
            if val == "* Confirm selection" then
              validated = true
            else
              table.insert(results, val)
            end
          end
          if not validated then
            results = {}
          end
          if spec.on_confirm then
            vim.schedule(function()
              spec.on_confirm(results)
            end)
          end
        end)
        -- local default_check = spec.default_selected and true or false
        -- if default_check then
        -- 	vim.schedule(function()
        -- 		actions.select_all(prompt_bufnr)
        -- 	end)
        -- end
        return true
      end
    end,
  },
}
behaviour.multiselect_empty.loop = behaviour.multiselect.loop

local prepare_source = function(spec)
  local source = spec.source or { type = "static", items = spec.items }

  if source.type == "static" then
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
      make_display = function(entry)
        return displayer({ { entry.icon, entry.icon_hl }, entry.display_text })
      end
    end

    local finder = finders.new_table({
      results = source.items or {},
      entry_maker = to_telescope_item(use_devicons, devicons, make_display),
    })

    local picker_opts = {
      prompt_title = spec.title or "Select",
      finder = finder,
      sorter = sorters.get_generic_fuzzy_sorter({}),
      file_ignore_patterns = {},
      handle_item = function(item)
        return item and item.value or nil
      end,
    }

    if spec.preview_enabled ~= false then
      local use_grep_previewer = false
      if spec.preview_mode == "grep" then
        use_grep_previewer = true
      elseif source.items and #source.items > 0 then
        -- Check first few items for line info
        for i = 1, math.min(5, #source.items) do
          local item = source.items[i]
          if type(item) == "table" and (item.lnum or item.line or item.row) then
            use_grep_previewer = true
            break
          end
        end
      end
      picker_opts.previewer = use_grep_previewer and conf.grep_previewer({}) or conf.file_previewer({})
    end

    return {
      opts = picker_opts,
      picker = function(opts)
        pickers.new(opts):find()
      end,
    }
  elseif source.type == "grep" then
    local builtin = require("telescope.builtin")
    local make_entry = require("telescope.make_entry")
    local entry_display = require("telescope.pickers.entry_display")
    local devicons_ok, devicons = pcall(require, "nvim-web-devicons")
    local use_devicons = spec.devicons_enabled ~= false and devicons_ok

    local displayer, make_display
    if use_devicons then
      displayer = entry_display.create({ separator = " ", items = { { width = 2 }, { remaining = true } } })
      make_display = function(entry)
        return displayer({ { entry.icon, entry.icon_hl }, entry.display_text })
      end
    end

    local args = {}
    for _, dir in ipairs(source.exclude_directories or {}) do
      table.insert(args, "--glob")
      table.insert(args, "!" .. dir)
    end
    for _, ext in ipairs(source.include_extensions or {}) do
      table.insert(args, "-g")
      table.insert(args, "*." .. ext)
    end

    local grep_opts = {
      prompt_title = spec.title or "Live Grep",
      search_dirs = source.search_paths,
      additional_args = args,
      handle_item = function(item)
        return {
          filename = item.filename,
          lnum = item.lnum,
          col = item.col,
        }
      end,
    }

    if spec.transform_display or use_devicons then
      local default_maker = make_entry.gen_from_vimgrep(spec)
      grep_opts.entry_maker = function(line)
        local entry = default_maker(line)
        if not entry then
          return nil
        end
        local disp_path = (spec.transform_display and spec.transform_display(entry.filename)) or entry.filename
        local disp_text = string.format("%s:%s:%s:%s", disp_path, entry.lnum, entry.col, entry.text)
        if use_devicons then
          local icon, hl = devicons.get_icon(entry.filename, vim.fn.fnamemodify(entry.filename, ":e"))
          entry.display, entry.icon, entry.icon_hl, entry.display_text =
            make_display, icon or "", hl or "Normal", disp_text
        else
          entry.display = disp_text
        end
        return entry
      end
    end

    return {
      opts = grep_opts,
      picker = builtin.live_grep,
    }
  elseif source.type == "callback" then
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
      make_display = function(entry)
        return displayer({ { entry.icon, entry.icon_hl }, entry.display_text })
      end
    end

    local entry_maker = to_telescope_item(use_devicons, devicons, make_display)

    local finder = setmetatable({ results = results, close = function() end }, {
      __call = function(_, _, cb, cb_complete)
        for _, item in ipairs(results) do
          local e = entry_maker(item)
          if e then
            cb(e)
          end
        end
        if cb_complete then
          cb_complete()
        end
      end,
    })

    local picker_opts = {
      prompt_title = spec.title or "Dynamic Picker",
      finder = finder,
      sorter = conf.generic_sorter({}),
      previewer = (spec.preview_enabled ~= false) and conf.file_previewer({}) or nil,
      sorting_strategy = "ascending",
      handle_item = function(item)
        return item and item.value or nil
      end,
    }

    return {
      opts = picker_opts,
      picker = function(opts)
        local picker = pickers.new(opts)
        picker.tiebreak = function()
          return false
        end
        picker:find()

        local push = function(items)
          if not items then
            return
          end
          local to_add = (type(items) == "table" and items[1] ~= nil) and items or { items }
          for _, it in ipairs(to_add) do
            table.insert(results, it)
          end

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

        if source.fn then
          source.fn(push)
        end
      end,
    }
  elseif source.type == "job" then
    local finders = require("telescope.finders")
    local pickers = require("telescope.pickers")
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
      handle_item = function(item)
        return item and item.value or nil
      end,
    }

    if spec.preview_enabled then
      picker_opts.previewer = conf.file_previewer({})
    end

    return {
      opts = picker_opts,
      picker = function(opts)
        pickers.new(opts):find()
      end,
    }
  end
end

function M.run(spec)
  local logging = require("UNL.logging")
  local log = logging.get(spec.logger_name or "UNL")
  spec = spec or {}
  local picker = prepare_source(spec)
  local mode
  if (not spec.multiselect) or spec.multiselect == "single" then
    mode = "single"
  elseif spec.multiselect == true or spec.multiselect == "multiselect" then
    mode = "multiselect"
  elseif spec.multiselect == "multiselect_empty" then
    mode = "multiselect_empty"
  else
    log.error(
      "Unknown value for multiselect: %s. Should be 'single', 'multiselect', 'multiselect_empty'.",
      spec.multiselect
    )
    return
  end

  if type(spec.conf.ui.picker.behaviour[mode]) == "function" then
    spec.conf.ui.picker.behaviour[mode](picker, spec)
  elseif
    type(spec.conf.ui.picker.behaviour[mode]) == "string"
    and behaviour[mode][spec.conf.ui.picker.behaviour[mode]]
    and type(behaviour[mode][spec.conf.ui.picker.behaviour[mode]]) == "function"
  then
    behaviour[mode][spec.conf.ui.picker.behaviour[mode]](picker, spec)
  else
    log.error("Unknown behaviour '%s' for multiselect mode '%s'.", spec.conf.ui.picker.behaviour[mode], mode)
    return
  end
  picker.picker(picker.opts)
end

return M
