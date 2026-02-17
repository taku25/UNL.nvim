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

function M.run_static(spec, source)
  local Snacks = require("snacks")
  local items = {}
  for _, it in ipairs(source.items or {}) do
    table.insert(items, to_snacks_item(it))
  end

  Snacks.picker.pick({
    title = spec.title or "Select",
    items = items,
    format = "file",
    preview = (spec.preview_enabled ~= false) and "file" or "none",
    confirm = function(picker, item)
      if not item then
        return
      end
      picker:close()
      if spec.on_confirm then
        vim.schedule(function()
          local is_multi = (spec.multiselect == "native" or spec.multiselect == true)
          if is_multi then
            local sel = picker:selected()
            if #sel == 0 then
              sel = { item }
            end
            local res = {}
            for _, s in ipairs(sel) do
              table.insert(res, s.value)
            end
            spec.on_confirm(res)
          else
            spec.on_confirm(item.value)
          end
        end)
      end
    end,
  })
end

function M.run_grep(spec, source)
  local Snacks = require("snacks")
  Snacks.picker.grep({
    title = spec.title or "Live Grep",
    dirs = source.search_paths and vim.tbl_map(normalize_path, source.search_paths) or nil,
    exclude = source.exclude_directories,
    confirm = function(picker, item)
      if item and item.file and item.pos then
        picker:close()
        if spec.on_confirm then
          vim.schedule(function()
            spec.on_confirm({ filename = normalize_path(item.file), lnum = item.pos[1], col = item.pos[2] })
          end)
        end
      else
        picker:close()
      end
    end,
  })
end

function M.run_callback(spec, source)
  local Snacks = require("snacks")

  Snacks.picker.pick({
    title = spec.title or "Dynamic Picker",
    format = "file",
    preview = (spec.preview_enabled ~= false) and "file" or "none",
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
    confirm = function(picker, item)
      if not item then
        return
      end
      picker:close()
      if spec.on_confirm then
        vim.schedule(function()
          local is_multi = (spec.multiselect == "native" or spec.multiselect == true)
          if is_multi then
            local sel = picker:selected()
            if #sel == 0 then
              sel = { item }
            end
            local res = {}
            for _, s in ipairs(sel) do
              table.insert(res, s.value)
            end
            spec.on_confirm(res)
          else
            spec.on_confirm(item.value)
          end
        end)
      end
    end,
  })
end

function M.run_job(spec, source)
  local Snacks = require("snacks")
  local cmd = source.command
  local args = {}
  if type(cmd) == "table" then
    for i = 2, #cmd do
      table.insert(args, cmd[i])
    end
    cmd = cmd[1]
  end
  Snacks.picker.pick({
    title = spec.title or "Find",
    finder = "proc",
    cmd = cmd,
    args = args,
    cwd = normalize_path(spec.cwd),
    confirm = function(picker, item)
      if item and spec.on_confirm then
        picker:close()
        vim.schedule(function()
          spec.on_confirm(item.text)
        end)
      end
    end,
  })
end

return M
