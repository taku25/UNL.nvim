-- lua/UNL/backend/picker/provider/snacks.lua
-- Unified Snacks picker provider for UNL.nvim

local M = { name = "snacks" }

function M.available()
  return pcall(require, "snacks")
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
  local Snacks = require("snacks")
  local cmd = source.command
  if type(cmd) == "table" then cmd = table.concat(cmd, " ") end

  Snacks.picker.pick({
    title = spec.title or "Find",
    cmd = cmd,
    cwd = spec.cwd,
    actions = {
      confirm = function(picker, item)
        if item and spec.on_confirm then
          Snacks.picker.actions.close(picker)
          vim.schedule(function() spec.on_confirm(item.text or item[1]) end)
        end
      end
    }
  })
end

function M.run_static(spec, source)
  local Snacks = require("snacks")
  local devicons_ok, devicons = pcall(require, "nvim-web-devicons")
  local items = source.items or {}

  for _, item in ipairs(items) do
    if type(item) == "table" then
      item.value = item.value or item
      item.display = item.display or item.label or item.name or tostring(item.value)
      item.filename = item.filename or (type(item.value) == "table" and item.value.filename)
      local l, c = item.lnum or item.line or item.row, item.col or 0
      if l then item.pos = { tonumber(l), tonumber(c) } end
      if item.filename and not item.file then item.file = item.filename end
      if type(item.value) == "table" and string.match(item.display, "^table: 0x") then
        item.display = item.value.display or item.value.label or item.value.name or item.display
      end
      if not item.text then item.text = item.display or (type(item.value) == "string" and item.value) or item.file or "" end
    end
  end

  local opts = {
    title = spec.title or "Select",
    items = items,
    multi = (spec.multiselect == "native" or spec.multiselect == true),
    format = function(item)
      local highlights = {}
      if spec.devicons_enabled and devicons_ok and item.file then
        local icon, hl = devicons.get_icon(item.file, vim.fn.fnamemodify(item.file, ":e"))
        if icon then table.insert(highlights, { icon .. " ", hl or "Normal" }) end
      end
      table.insert(highlights, { item.display or item.text or "" })
      return highlights
    end,
    actions = {
      confirm = function(picker, item)
        if not item then return end
        Snacks.picker.actions.close(picker)
        if spec.on_confirm then
          vim.schedule(function()
            local is_multi = (spec.multiselect == "native" or spec.multiselect == true)
            if is_multi then
              local sel = picker:selected()
              if #sel == 0 then sel = {item} end
              local res = {}
              for _, s in ipairs(sel) do table.insert(res, s.value or s) end
              spec.on_confirm(res)
            else
              spec.on_confirm(item.value or item)
            end
          end)
        end
      end
    }
  }

  if spec.preview_enabled ~= false then opts.preview = "file" else opts.layout = { hidden = { "preview" } } end
  Snacks.picker.pick(opts)
end

function M.run_grep(spec, source)
  local Snacks = require("snacks")
  local opts = {
    title = spec.title or "Live Grep",
    dirs = source.search_paths,
    exclude = source.exclude_directories,
    actions = {
      confirm = function(picker, item)
        if item and item.file and item.pos then
          Snacks.picker.actions.close(picker)
          if spec.on_confirm then
            vim.schedule(function() spec.on_confirm({ filename = item.file, lnum = item.pos[1], col = item.pos[2] }) end)
          end
        else
          Snacks.picker.actions.close(picker)
        end
      end
    }
  }
  if source.include_extensions and #source.include_extensions > 0 then
    opts.glob = vim.tbl_map(function(ext) return "*." .. ext end, source.include_extensions)
  end
  Snacks.picker.grep(opts)
end

function M.run_callback(spec, source)
  -- Snacks doesn't have a direct "dynamic push" API in the same way, 
  -- but we can use a custom source. For simplicity, we'll implement it if needed or use static as fallback.
  -- For now, let's use the static picker approach but with a source function if possible.
  local Snacks = require("snacks")
  
  local opts = {
    title = spec.title or "Dynamic Picker",
    source = function(p, cb)
      local push = function(items)
        if not items then return end
        local to_add = (type(items) == "table" and items[1] ~= nil) and items or {items}
        local formatted = {}
        for _, it in ipairs(to_add) do
          table.insert(formatted, {
            text = (type(it) == "table") and (it.display or it.label or it.name or tostring(it.value)) or tostring(it),
            value = (type(it) == "table" and it.value) or it
          })
        end
        cb(formatted)
      end
      if source.fn then source.fn(push) end
    end,
    actions = {
      confirm = function(picker, item)
        if item and spec.on_confirm then
          Snacks.picker.actions.close(picker)
          vim.schedule(function() spec.on_confirm(item.value or item) end)
        end
      end
    }
  }
  Snacks.picker.pick(opts)
end

return M