-- lua/UNL/backend/picker/provider/fzf_lua.lua
-- Unified fzf-lua provider for UNL.nvim

local M = { name = "fzf-lua" }

function M.available()
  return pcall(require, "fzf-lua")
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

function M.run_job(spec, source)
  local fzf = require("fzf-lua")
  local cmd = source.command
  if type(cmd) == "table" then cmd = table.concat(cmd, " ") end
  
  fzf.fzf_exec(cmd, {
    prompt = spec.title or "Find> ",
    cwd = spec.cwd or vim.loop.cwd(),
    actions = {
      ["default"] = function(selected)
        if selected and #selected > 0 and spec.on_confirm then
          vim.schedule(function() spec.on_confirm(selected[1]) end)
        end
      end
    }
  })
end

function M.run_static(spec, source)
  local fzf = require("fzf-lua")
  local builtin = require("fzf-lua.previewer.builtin")
  
  local display_items = {}
  local lookup = {}

  for _, item in ipairs(source.items or {}) do
    local value, display, filename, lnum, col
    if type(item) == 'table' then
      value, display = item.value or item, item.display or item.label or item.name or tostring(item.value or item)
      filename, lnum, col = item.filename or item.file_path, item.lnum or item.line or item.row, item.col
    else
      value, display, filename = item, tostring(item), tostring(item)
    end
    
    local processed = { value = value, filename = filename, lnum = tonumber(lnum), col = tonumber(col) }
    table.insert(display_items, display)
    lookup[display] = processed
  end

  local opts = {
    prompt = spec.title or "Select Item> ",
    cwd = spec.cwd or vim.loop.cwd(),
    multiselect = (spec.multiselect == "native" or spec.multiselect == true),
    actions = {
      ["default"] = function(selected)
        if not selected or #selected == 0 then return end
        local results = {}
        for _, key in ipairs(selected) do if lookup[key] then table.insert(results, lookup[key].value) end end
        if spec.on_confirm then
          local is_multi = (spec.multiselect == "native" or spec.multiselect == true)
          vim.schedule(function() spec.on_confirm(is_multi and results or results[1]) end)
        end
      end
    }
  }

  if spec.preview_enabled ~= false then
    local Prev = builtin.buffer_or_file:extend()
    function Prev:new(o, opts, f_win) Prev.super.new(self, o, opts, f_win); setmetatable(self, Prev); return self end
    function Prev:parse_entry(entry)
      local it = lookup[entry]
      return it and { path = it.filename, line = it.lnum, col = it.col } or {}
    end
    opts.previewer = Prev
  end

  fzf.fzf_exec(display_items, opts)
end

function M.run_grep(spec, source)
  local fzf = require("fzf-lua")
  local args = { "--vimgrep", "--line-number", "--column", "--smart-case", "--no-heading", "--hidden" }
  for _, dir in ipairs(source.exclude_directories or {}) do table.insert(args, "--glob"); table.insert(args, "!" .. dir) end
  for _, ext in ipairs(source.include_extensions or {}) do table.insert(args, "-g"); table.insert(args, "*." .. ext) end
  
  fzf.live_grep({
    prompt = spec.title or "Live Grep> ",
    search_dirs = source.search_paths,
    rg_opts = table.concat(args, " "),
    actions = {
      ["default"] = function(selected)
        local e = selected[1]
        if not e then return end
        local f, l, c = e:match("^([^:]+):(%d+):(%d+):.*$")
        if f and l and spec.on_confirm then
          vim.schedule(function() spec.on_confirm({ filename = f, lnum = tonumber(l), col = tonumber(c) }) end)
        end
      end
    }
  })
end

function M.run_callback(spec, source)
  local fzf = require("fzf-lua")
  local fzf_fn = function(cb)
    local push = function(items)
      if not items then return end
      local to_add = (type(items) == "table" and items[1] ~= nil) and items or {items}
      for _, it in ipairs(to_add) do
        local line = (type(it) == "table") and (it.display or it.label or it.filename or tostring(it.value)) or tostring(it)
        cb(line)
      end
    end
    if source.fn then source.fn(push) end
  end

  local opts = {
    prompt = (spec.title or "Stack") .. "> ",
    actions = {
      ["default"] = function(selected)
        if selected and #selected > 0 and spec.on_confirm then
          vim.schedule(function() spec.on_confirm(selected[1]) end)
        end
      end
    }
  }

  if spec.preview_enabled ~= false then
    opts.previewer = "builtin" -- Use builtin previewer for common patterns (file:line)
  end

  fzf.fzf_exec(fzf_fn, opts)
end

return M