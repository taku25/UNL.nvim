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

local function normalize_path(p)
  if not p then return nil end
  return p:gsub("\\", "/"):gsub("//+", "/")
end

local function get_safe_cwd(spec_cwd)
  return normalize_path(spec_cwd or vim.fn.getcwd())
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
    if not path:match("^%a:/") and not path:starts_with("/") then
      path = cwd .. "/" .. path
    end
  end
  
  return path, line, col
end

function M.run_static(spec, source)
  local fzf = require("fzf-lua")
  local builtin = require("fzf-lua.previewer.builtin")
  
  local display_items = {}
  local lookup = {}
  local cwd = get_safe_cwd(spec.cwd)

  for _, item in ipairs(source.items or {}) do
    local value, display, filename, lnum, col
    if type(item) == 'table' then
      value, display = item.value or item, item.display or item.label or item.name or tostring(item.value or item)
      filename, lnum, col = normalize_path(item.filename or item.file_path), item.lnum or item.line or item.row, item.col
    else
      value, display, filename = item, tostring(item), normalize_path(tostring(item))
    end
    
    lookup[display] = { value = value, filename = filename, lnum = tonumber(lnum), col = tonumber(col) }
    table.insert(display_items, display)
  end

  local opts = {
    prompt = spec.title or "Select Item> ",
    cwd = cwd,
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
      local path, line, col = extract_path_from_entry(entry, lookup, cwd)
      return path and { path = path, line = line, col = col } or {}
    end
    opts.previewer = Prev
  end

  fzf.fzf_exec(display_items, opts)
end

function M.run_grep(spec, source)
  local fzf = require("fzf-lua")
  local args = { "--vimgrep", "--line-number", "--column", "--smart-case", "--no-heading", "--hidden" }
  for _, dir in ipairs(source.exclude_directories or {}) do table.insert(args, "--glob"); table.insert(args, "!" .. normalize_path(dir)) end
  for _, ext in ipairs(source.include_extensions or {}) do table.insert(args, "-g"); table.insert(args, "*." .. ext) end
  
  local search_paths = {}
  for _, p in ipairs(source.search_paths or {}) do table.insert(search_paths, normalize_path(p)) end

  fzf.live_grep({
    prompt = spec.title or "Live Grep> ",
    cwd = get_safe_cwd(spec.cwd),
    search_dirs = search_paths,
    rg_opts = table.concat(args, " "),
    actions = {
      ["default"] = function(selected)
        local e = selected[1]
        if not e then return end
        local f, l, c = e:match("^([^:]+):(%d+):(%d+):.*$")
        if f and l and spec.on_confirm then
          vim.schedule(function() spec.on_confirm({ filename = normalize_path(f), lnum = tonumber(l), col = tonumber(c) }) end)
        end
      end
    }
  })
end

function M.run_callback(spec, source)
  local fzf = require("fzf-lua")
  local builtin = require("fzf-lua.previewer.builtin")
  local cwd = get_safe_cwd(spec.cwd)

  local fzf_fn = function(fzf_cb)
    local push = function(items)
      if not items then return end
      local to_add = (type(items) == "table" and items[1] ~= nil) and items or {items}
      for _, it in ipairs(to_add) do
        -- callback時は 'value' をそのまま渡す。UEPは value に "label\tpath" を入れているため
        local line = (type(it) == "table") and (it.value or it.display or it.label or it.filename) or tostring(it)
        fzf_cb(line)
      end
    end
    if source.fn then source.fn(push) end
  end

  local opts = {
    prompt = (spec.title or "Stack") .. "> ",
    cwd = cwd,
    actions = {
      ["default"] = function(selected)
        if selected and #selected > 0 and spec.on_confirm then
          -- UEP形式のタブ区切りから実パスを抽出
          local val = selected[1]:match("[^\t]+$") or selected[1]
          vim.schedule(function() spec.on_confirm(val) end)
        end
      end
    }
  }

  if spec.preview_enabled ~= false then
    local Prev = builtin.buffer_or_file:extend()
    function Prev:new(o, opts, f_win) Prev.super.new(self, o, opts, f_win); setmetatable(self, Prev); return self end
    function Prev:parse_entry(entry)
      -- ルックアップなしで文字列からパスを抽出
      local path, line, col = extract_path_from_entry(entry, nil, cwd)
      return path and { path = path, line = line, col = col } or {}
    end
    opts.previewer = Prev
  end

  fzf.fzf_exec(fzf_fn, opts)
end

function M.run_job(spec, source)
  local fzf = require("fzf-lua")
  local cmd = source.command
  if type(cmd) == "table" then
    local normalized_cmd = {}
    for _, arg in ipairs(cmd) do table.insert(normalized_cmd, arg:match("[/\\]") and normalize_path(arg) or arg) end
    cmd = table.concat(normalized_cmd, " ")
  end
  
  fzf.fzf_exec(cmd, {
    prompt = spec.title or "Find> ",
    cwd = get_safe_cwd(spec.cwd),
    actions = {
      ["default"] = function(selected)
        if selected and #selected > 0 and spec.on_confirm then
          vim.schedule(function() spec.on_confirm(selected[1]) end)
        end
      end
    }
  })
end

return M