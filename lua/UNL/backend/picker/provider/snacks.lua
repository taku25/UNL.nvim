-- lua/UNL/backend/picker/provider/snacks.lua
-- Unified Snacks picker provider for UNL.nvim, with robust async support

local M = { name = "snacks" }

function M.available()
  local ok, snacks = pcall(require, "snacks")
  return ok and snacks.picker ~= nil
end

local function normalize_path(p)
  if not p then
    return nil
  end
  return p:gsub("\\", "/"):gsub("//+", "/")
end

-- アイテムを Snacks 形式に変換
local function to_snacks_item(item)
  local path, lnum, col
  local label = ""
  local value = item

  if type(item) == "table" then
    path = item.filename or item.file_path or item.file
    lnum = item.lnum or item.line or item.row
    col = item.col
    label = item.display or item.label or item.name or ""
    value = item.value or item
    local val_str = tostring(item.value or "")
    if val_str:find("\t") then
      path = val_str:match("[^\t]+$")
    end
  else
    label = tostring(item)
    path = label:match("[^\t]+$") or label
  end

  if path then
    path = path:match("^(.*) %b()$") or path
    path = normalize_path(path)
  end

  return {
    text = label ~= "" and label or (path and vim.fn.fnamemodify(path, ":t")) or "Unknown",
    file = path,
    pos = lnum and { tonumber(lnum), tonumber(col or 0) } or nil,
    value = value,
  }
end

local behaviour = {
  single = {
    native = function(picker, spec)
      local opts = picker.opts
      opts.confirm = function(picker, item)
        picker:close()
        if spec.on_confirm and item then
          vim.schedule(function()
            spec.on_confirm(opts.handle_item(item))
          end)
        end
      end
    end,
  },
  multiselect = {
    native = function(picker, spec)
      local opts = picker.opts
      opts.confirm = function(picker, item)
        picker:close()
        if spec.on_confirm and item then
          vim.schedule(function()
            local sel = picker:selected()
            if #sel == 0 then
              sel = { item }
            end
            local res = {}
            for _, s in ipairs(sel) do
              table.insert(res, opts.handle_item(s))
            end
            spec.on_confirm(res)
          end)
        end
      end

      local default_check = spec.default_selected and true or false
      if default_check then
        opts.on_show = function(picker)
          picker.list:select_all()
        end
      end
    end,
    loop = function(picker, spec)
      local opts = picker.opts
      if opts.items then
        local default_check = spec.default_selected and true or false
        for _, item in ipairs(opts.items) do
          if not spec.devicons_enabled then
            item.base = item.text
            item.checked = default_check
            item.text = (item.checked and "󰄲 " or " ") .. item.base
          else
            item.base = item.file
            item.checked = default_check
            item.file = (item.checked and "󰄲 " or " ") .. item.base
          end
        end
        table.insert(opts.items, 1, to_snacks_item("* Confirm selection"))
      end
      opts.confirm = function(picker, item)
        if opts.handle_item(item) == "* Confirm selection" then
          picker:close()
          if spec.on_confirm and item then
            vim.schedule(function()
              local res = {}
              for _, list_item in ipairs(picker.list.items) do
                if list_item.checked then
                  table.insert(res, opts.handle_item(list_item))
                end
              end
              spec.on_confirm(res)
            end)
          end
        else
          item.checked = not item.checked
          if not spec.devicons_enabled then
            item.text = (item.checked and "󰄲 " or " ") .. item.base
          else
            item.file = (item.checked and "󰄲 " or " ") .. item.base
            item._path = (item.checked and "󰄲 " or " ") .. item.base
          end
          picker.list:update({ force = true })
        end
      end
    end,
  },
  multiselect_empty = {
    confirm_item = function(picker, spec)
      local opts = picker.opts
      if opts.items then
        table.insert(opts.items, 1, to_snacks_item("* Confirm selection"))
      end
      opts.confirm = function(picker, item)
        picker:close()
        if spec.on_confirm and item then
          vim.schedule(function()
            local sel = picker:selected()
            if sel then
              local select_res = {}
              local select_valid = false
              for _, val in ipairs(sel) do
                if opts.handle_item(val) == "* Confirm selection" then
                  select_valid = true
                else
                  table.insert(select_res, opts.handle_item(val))
                end
              end
              if not select_valid then
                select_res = {}
              end
              spec.on_confirm(select_res)
            end
          end)
        end
      end

      local default_check = spec.default_selected and true or false
      if default_check then
        opts.on_show = function(picker)
          picker.list:select_all()
        end
      end
    end,
    native = function(picker, spec)
      local opts = picker.opts
      opts.confirm = function(picker, item)
        picker:close()
        if spec.on_confirm and item then
          vim.schedule(function()
            local sel = picker:selected()
            local res = {}
            for _, s in ipairs(sel) do
              table.insert(res, opts.handle_item(s))
            end
            spec.on_confirm(res)
          end)
        end
      end

      local default_check = spec.default_selected and true or false
      if default_check then
        opts.on_show = function(picker)
          picker.list:select_all()
        end
      end
    end,
  },
}
behaviour.multiselect_empty.loop = behaviour.multiselect.loop

local function prepare_source(spec)
  local source = spec.source or { type = "static", items = spec.items }
  local Snacks = require("snacks")

  if source.type == "static" then
    local items = {}
    for _, it in ipairs(source.items or {}) do
      table.insert(items, to_snacks_item(it))
    end

    local snacks_opts = {
      title = spec.title or "Select",
      items = items,
      format = spec.devicons_enabled and "file" or "text",
      handle_item = function(item)
        if item and spec.on_confirm then
          return item.value
        end
      end,
    }

    if spec.preview_enabled ~= false then
      snacks_opts.preview = "file"
    end

    if spec.preview_enabled == false then
      snacks_opts.layout = { hidden = { "preview" } }
    end

    return {
      opts = snacks_opts,
      picker = Snacks.picker.pick,
    }
  elseif source.type == "grep" then
    local snacks_opts = {
      title = spec.title or "Live Grep",
      dirs = source.search_paths and vim.tbl_map(normalize_path, source.search_paths) or nil,
      exclude = source.exclude_directories,
      format = spec.devicons_enabled and "file" or "text",
      handle_item = function(item)
        if item and item.file and item.pos and spec.on_confirm then
          return {
            filename = normalize_path(item.file),
            lnum = item.pos[1],
            col = item.pos[2],
          }
        end
      end,
    }

    return {
      opts = snacks_opts,
      picker = Snacks.picker.grep,
    }
  elseif source.type == "callback" then
    local snacks_opts = {
      title = spec.title or "Dynamic Picker",
      format = spec.devicons_enabled and "file" or "text",
      finder = function(_, ctx)
        return function(cb)
          local task = ctx.async
          local push = function(items)
            if task:aborted() then
              return
            end
            local to_add = (type(items) == "table" and items[1] ~= nil) and items or { items }
            for _, it in ipairs(to_add) do
              cb(to_snacks_item(it))
            end
            -- データを流し込んだ後にタスクをレジュームして画面を更新させる
            task:resume()
          end

          if source.fn then
            source.fn(push)
          end

          -- ピッカーが閉じられるまでタスクを終了させず、サスペンド状態で待機する
          while not task:aborted() do
            task:suspend()
          end
        end
      end,
      handle_item = function(item)
        if item and spec.on_confirm then
          return item.value
        end
      end,
    }

    if spec.preview_enabled ~= false then
      snacks_opts.preview = "file"
    end

    if spec.preview_enabled == false then
      snacks_opts.layout = { hidden = { "preview" } }
    end

    return {
      opts = snacks_opts,
      picker = Snacks.picker.pick,
    }
  elseif source.type == "job" then
    local cmd = source.command
    local args = {}
    if type(cmd) == "table" then
      for i = 2, #cmd do
        table.insert(args, cmd[i])
      end
      cmd = cmd[1]
    end
    local snacks_opts = {
      title = spec.title or "Find",
      finder = "proc",
      format = spec.devicons_enabled and "file" or "text",
      cmd = cmd,
      args = args,
      cwd = normalize_path(spec.cwd),
      handle_item = function(item)
        if item and spec.on_confirm then
          return item.text
        end
      end,
    }

    return {
      opts = snacks_opts,
      picker = Snacks.picker.pick,
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
