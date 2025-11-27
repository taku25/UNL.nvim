-- lua/UNL/backend/picker/provider/fzf_lua.lua

local M = { name = "fzf-lua" }

function M.available()
  return pcall(require, "fzf-lua")
end

function M.run(spec)
  local fzf_lua = require("fzf-lua")
  local log = require("UNL.logging").get("UNL")
  local builtin = require("fzf-lua.previewer.builtin")
  spec = spec or {}

  local display_items = {}
  local display_to_processed_item = {}

  if spec.items then
    local entry_maker_to_use = spec.entry_maker or function(item)
      local value, display, filename, lnum, col
      if type(item) == 'table' then
        value = item.value or item
        display = item.display or item.label or item.name or tostring(value)
        filename = item.filename or item.file_path
        
        -- ★★★ 修正: 行番号・列番号のエイリアスに対応 ★★★
        lnum = item.lnum or item.line or item.row
        col = item.col
      else
        value = item
        display = tostring(item)
        filename = tostring(item)
      end
      if type(value) == 'table' and string.match(display, "^table: 0x") then
        display = value.display or value.label or value.name or display
      end
      
      -- 数値型に変換しておく
      if lnum then lnum = tonumber(lnum) end
      if col then col = tonumber(col) end

      return { value = value, display = display, filename = filename, lnum = lnum, col = col }
    end

    for _, item in ipairs(spec.items) do
      local processed = entry_maker_to_use(item)
      local display_key = processed.display or ""
      table.insert(display_items, display_key)
      display_to_processed_item[display_key] = processed
    end
  end

  local fzf_opts = {
    prompt = spec.title or "Select Item> ",
    cwd = spec.cwd or vim.loop.cwd(),
    -- FzfLuaはデフォルトでマルチ選択が可能だが、呼び出し元の意図に合わせて制御したい場合はここで行う
    -- (fzf-luaのAPI仕様上、完全にシングルモードにするオプションはないが、header等で案内は可能)
    actions = {
      ["default"] = function(selected_list)
        if not selected_list then selected_list = {} end
        
        local results = {}
        for _, display_key in ipairs(selected_list) do
          local item = display_to_processed_item[display_key]
          if item then table.insert(results, item.value) end
        end

        if spec.on_submit then
          if spec.multi_select then
            vim.schedule(function() spec.on_submit(results) end)
          else
            vim.schedule(function() spec.on_submit(#results > 0 and results[1] or nil) end)
          end
        end
      end,
      ["ctrl-c"] = function() if spec.on_cancel then vim.schedule(spec.on_cancel) end end,
    },
  }

  if spec.preview_enabled ~= false then
    local GenericFzfPreviewer = builtin.buffer_or_file:extend()

    function GenericFzfPreviewer:new(o, opts, fzf_win)
      GenericFzfPreviewer.super.new(self, o, opts, fzf_win)
      setmetatable(self, GenericFzfPreviewer)
      return self
    end
    
    function GenericFzfPreviewer:parse_entry(entry_str)
      local item = display_to_processed_item[entry_str]
      if item and item.filename and type(item.filename) == 'string' then
        return {
          path = item.filename,
          line = item.lnum, -- ここにはエイリアス解決済みの値が入っている
          col = item.col,
        }
      end
      return {}
    end
    
    fzf_opts.previewer = GenericFzfPreviewer
  end

  if #display_items > 0 then
    fzf_lua.fzf_exec(display_items, fzf_opts)
  elseif spec.exec_cmd then
    fzf_lua.fzf_exec(spec.exec_cmd, fzf_opts)
  else
    log.warn("fzf-lua provider: No items or exec_cmd provided.")
  end
end

return M
