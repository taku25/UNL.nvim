-- lua/UNL/backend/picker/provider/fzf_lua.lua
-- Unified fzf-lua provider for UNL.nvim

local M = { name = "fzf-lua" }

function M.available()
  return pcall(require, "fzf-lua")
end

local function normalize_path(p)
  if not p then
    return nil
  end
  return p:gsub("\\", "/"):gsub("//+", "/")
end

local function get_safe_cwd(spec_cwd)
  return normalize_path(spec_cwd or vim.fn.getcwd())
end

local function to_fzf_item(item, lookup)
  local value, display, filename, lnum, col
  if type(item) == "table" then
    value, display = item.value or item, item.display or item.label or item.name or tostring(item.value or item)
    filename, lnum, col =
      normalize_path(item.filename or item.file_path), item.lnum or item.line or item.row, item.col
  else
    value, display, filename = item, tostring(item), normalize_path(tostring(item))
  end
  lookup[display] = { value = value, filename = filename, lnum = tonumber(lnum), col = tonumber(col) }
end

-- 汎用的なパス抽出ロジック (Display文字列から実パスを取り出す)
local function extract_path_from_entry(entry, lookup, cwd)
  local path, line, col

  -- 1. ルックアップテーブルにあるか確認 (static用)
  if lookup and lookup[entry] then
    path, line, col = lookup[entry].filename, lookup[entry].lnum, lookup[entry].col
  else
    -- 2. 文字列から推測 (callback/job用)
    -- タブ区切りがある場合、後半がパス
    path = entry:match("[^\t]+$")
    if not path or path == entry then
      -- "(Module)" サフィックスを除去
      path = entry:match("^(.*) %b()$") or entry
    end
  end

  if path then
    path = normalize_path(path)
    -- 相対パスならCWDを付与
    if not path:match("^%a:/") and not path:match("^/") then
      path = cwd .. "/" .. path
    end
  end

  return path, line, col
end

local behaviour = {
  single = {
    native = function(picker, spec)
      local opts = picker.opts
      if not opts.is_grep then
        opts.field_index = "{s}"
      end
      opts.actions = {
        ["default"] = function(selected)
          if not selected then
            return
          end
          local result = opts.handle_item(selected[1])
          if spec.on_confirm then
            vim.schedule(function()
              spec.on_confirm(result)
            end)
          end
        end,
      }
    end,
  },
  multiselect = {
    native = function(picker, spec)
      local opts = picker.opts
      opts.field_index = "{+}"
      if not opts["fzf_opts"] then
        opts["fzf_opts"] = {}
      end
      opts["fzf_opts"]["--multi"] = true
      opts.actions = {
        ["default"] = function(selected)
          if not selected or #selected == 0 then
            return
          end
          local results = {}
          for _, key in ipairs(selected) do
            table.insert(results, opts.handle_item(key))
          end
          if spec.on_confirm then
            vim.schedule(function()
              spec.on_confirm(results)
            end)
          end
        end,
      }

      local default_check = spec.default_selected and true or false
      if default_check then
        if not opts["keymap"] then
          opts["keymap"] = {}
        end
        if not opts["keymap"]["fzf"] then
          opts["keymap"]["fzf"] = {}
        end
        opts.keymap.fzf["load"] = "select-all"
      end
    end,
    loop = function(picker, spec)
      local opts = picker.opts
      local default_check = spec.default_selected and true or false
      if opts.display_items then
        if type(opts.display_items) == "function" then
          if opts.check_status == nil then
            opts.check_status = {}
          end
          local old_cb = opts.display_items
          opts.display_items = function(fzf_cb)
            local checked_wrapper = function(item)
              if opts.check_status[item] == nil then
                opts.check_status[item] = default_check
              end
              fzf_cb((opts.check_status[item] and "󰄲 " or " ") .. item)
            end
            to_fzf_item("* Confirm items", opts.lookup)
            fzf_cb("* Confirm items")
            old_cb(checked_wrapper)
          end
        elseif type(opts.display_items) == "table" then
          opts.display_items = {}
          for key, val in pairs(opts.lookup) do
            if key ~= "* Confirm items" then
              if val.checked == nil then
                val.checked = default_check
              end
              table.insert(opts.display_items, (val.checked and "󰄲 " or " ") .. key)
            end
          end
          if opts.lookup["* Confirm items"] == nil then
            to_fzf_item("* Confirm items", opts.lookup)
          end
          table.insert(opts.display_items, 1, "* Confirm items")
        end
      end
      opts.actions = {
        ["default"] = function(selected)
          if not selected then
            return
          end

          if selected[1] == "* Confirm items" then
            local res = {}
            if type(opts.display_items) == "function" then
              for key, _ in pairs(opts.lookup) do
                if opts.check_status[key] then
                  table.insert(res, opts.handle_item(key))
                end
              end
            elseif type(opts.display_items) == "table" then
              for key, val in pairs(opts.lookup) do
                if val.checked then
                  table.insert(res, opts.handle_item(key))
                end
              end
            end
            if spec.on_confirm then
              vim.schedule(function()
                spec.on_confirm(res)
              end)
            end
          else
            local item = selected[1]:match("[^ %s]* (.+)")
            if type(opts.display_items) == "function" then
              opts.check_status[item] = not opts.check_status[item]
            elseif type(opts.display_items) == "table" then
              opts.lookup[item].checked = not opts.lookup[item].checked
              opts.display_items = {}
              for key, val in pairs(opts.lookup) do
                if key ~= "* Confirm items" then
                  if val.checked == nil then
                    val.checked = default_check
                  end
                  table.insert(opts.display_items, (val.checked and "󰄲 " or " ") .. key)
                end
              end
              table.insert(opts.display_items, 1, "* Confirm items")
            end
            opts.__FZF_VERSION = nil
            opts.__call_fn = nil
            opts.__call_opts = nil
            opts.__resume_key = nil
            picker.picker(opts)
          end
        end,
      }
    end,
  },
  multiselect_empty = {
    confirm_item = function(picker, spec)
      local opts = picker.opts
      opts.field_index = "{+}"
      if not opts["fzf_opts"] then
        opts["fzf_opts"] = {}
      end
      opts["fzf_opts"]["--multi"] = true
      if opts.display_items then
        if type(opts.display_items) == "function" then
          opts.display_items = function(fzf_cb)
            to_fzf_item("* Confirm items", opts.lookup)
            fzf_cb("* Confirm items")
            opts.display_items(fzf_cb)
          end
        elseif type(opts.display_items) == "table" then
          to_fzf_item("* Confirm items", opts.lookup)
          table.insert(opts.display_items, 1, "* Confirm items")
        end
      end

      opts.actions = {
        ["default"] = function(selected)
          if not selected or #selected == 0 then
            return
          end
          local results = {}
          local conf_item = false
          for _, key in ipairs(selected) do
            if key == "* Confirm items" then
              conf_item = true
            else
              table.insert(results, opts.handle_item(key))
            end
          end
          if not conf_item then
            results = {}
          end
          if spec.on_confirm then
            vim.schedule(function()
              spec.on_confirm(results)
            end)
          end
        end,
      }

      local default_check = spec.default_selected and true or false
      if default_check then
        if not opts["keymap"] then
          opts["keymap"] = {}
        end
        if not opts["keymap"]["fzf"] then
          opts["keymap"]["fzf"] = {}
        end
        opts.keymap.fzf["load"] = "select-all"
      end
    end,
  },
}
behaviour.multiselect_empty.native = behaviour.multiselect_empty.confirm_item
behaviour.multiselect_empty.loop = behaviour.multiselect.loop

local function prepare_source(spec)
  local source = spec.source or { type = "static", items = spec.items }
  local fzf = require("fzf-lua")
  local builtin = require("fzf-lua.previewer.builtin")

  if source.type == "static" then
    local fzf_opts = {}
    fzf_opts.lookup = {}
    fzf_opts.display_items = {}
    local cwd = get_safe_cwd(spec.cwd)

    for _, item in ipairs(source.items or {}) do
      to_fzf_item(item, fzf_opts.lookup)
    end
    for key, _ in pairs(fzf_opts.lookup) do
      table.insert(fzf_opts.display_items, key)
    end

    fzf_opts.prompt = spec.title or "Select Item> "
    fzf_opts.cwd = cwd
    fzf_opts.handle_item = function(item)
      return fzf_opts.lookup[item].value
    end

    if spec.preview_enabled ~= false then
      local Prev = builtin.buffer_or_file:extend()
      function Prev:new(o, opts, f_win)
        Prev.super.new(self, o, opts, f_win)
        setmetatable(self, Prev)
        return self
      end
      function Prev:parse_entry(entry)
        local path, line, col = extract_path_from_entry(entry, fzf_opts.lookup, cwd)
        return path and { path = path, line = line, col = col } or {}
      end
      fzf_opts.previewer = Prev
    end

    return {
      picker = function(opts)
        fzf.fzf_exec(opts.display_items, opts)
      end,
      opts = fzf_opts,
    }
  elseif source.type == "grep" then
    local args = { "--vimgrep", "--line-number", "--column", "--smart-case", "--no-heading", "--hidden" }
    for _, dir in ipairs(source.exclude_directories or {}) do
      table.insert(args, "--glob")
      table.insert(args, "!" .. normalize_path(dir))
    end
    for _, ext in ipairs(source.include_extensions or {}) do
      table.insert(args, "-g")
      table.insert(args, "*." .. ext)
    end

    local search_paths = {}
    for _, p in ipairs(source.search_paths or {}) do
      local npath = normalize_path(p)
      table.insert(search_paths, npath)
    end

    local fzf_opts = {
      prompt = spec.title or "Live Grep> ",
      cwd = get_safe_cwd(spec.cwd),
      search_paths = search_paths,
      rg_opts = table.concat(args, " "),
      is_grep = true,
      handle_item = function(item)
        local _, f, l, c = item:match("^([^ %s/\\]+)[ %s]+([^:]+):(%d+):(%d+):.*$")
        if not f then
          f, l, c = item:match("^[ %s]*([^:]+):(%d+):(%d+):.*$")
        end
        return {
          filename = normalize_path(f),
          lnum = tonumber(l),
          col = tonumber(c),
        }
      end,
    }

    return {
      picker = fzf.live_grep,
      opts = fzf_opts,
    }
  elseif source.type == "callback" then
    local fzf_opts = {}
    fzf_opts.lookup = {}
    local cwd = get_safe_cwd(spec.cwd)

    fzf_opts.display_items = function(fzf_cb)
      local push = function(items)
        local push_lookup = {}
        if not items then
          return
        end
        local to_add = (type(items) == "table" and items[1] ~= nil) and items or { items }
        for _, it in ipairs(to_add) do
          to_fzf_item(it, push_lookup)
        end
        for key, val in pairs(push_lookup) do
          fzf_opts.lookup[key] = val
          fzf_cb(key)
        end
      end
      if source.fn then
        source.fn(push)
      end
    end

    fzf_opts.prompt = (spec.title or "Stack") .. "> "
    fzf_opts.cwd = cwd
    fzf_opts.handle_item = function(item)
      return fzf_opts.lookup[item].value
    end

    if spec.preview_enabled ~= false then
      local Prev = builtin.buffer_or_file:extend()
      function Prev:new(o, opts, f_win)
        Prev.super.new(self, o, opts, f_win)
        setmetatable(self, Prev)
        return self
      end
      function Prev:parse_entry(entry)
        -- ルックアップなしで文字列からパスを抽出
        local path, line, col = extract_path_from_entry(entry, fzf_opts.lookup, cwd)
        return path and { path = path, line = line, col = col } or {}
      end
      fzf_opts.previewer = Prev
    end

    return {
      picker = function(opts)
        fzf.fzf_exec(fzf_opts.display_items, opts)
      end,
      opts = fzf_opts,
    }
  elseif source.type == "job" then
    local cmd = source.command
    if type(cmd) == "table" then
      local normalized_cmd = {}
      for _, arg in ipairs(cmd) do
        table.insert(normalized_cmd, arg:match("[/\\]") and normalize_path(arg) or arg)
      end
      cmd = table.concat(normalized_cmd, " ")
    end

    local fzf_opts = {
      prompt = spec.title or "Find> ",
      cwd = get_safe_cwd(spec.cwd),
      handle_item = function(item)
        return item
      end,
    }

    return {
      picker = function(opts)
        fzf.fzf_exec(cmd, opts)
      end,
      opts = fzf_opts,
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
