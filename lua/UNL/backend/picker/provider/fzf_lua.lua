-- lua/UNL/backend/picker/provider/fzf_lua.lua (すべての知見を統合した最終版)

local M = { name = "fzf-lua" }

function M.available()
  return pcall(require, "fzf-lua")
end

function M.run(spec)
  local fzf_lua = require("fzf-lua")
  local fzf_actions = require("fzf-lua.actions")
  local log = require("UNL.logging").get("UNL")
  -- ★★★ あなたのコードにあった、唯一の正しい require ★★★
  local builtin = require("fzf-lua.previewer.builtin")
  spec = spec or {}

  local display_items = {}
  local display_to_processed_item = {}

  if spec.items then
    local entry_maker_to_use = spec.entry_maker or function(item)
      -- デフォルト entry_maker
      local display_text, item_value
      if type(item) == "table" then
        item_value = item.value or item.name or tostring(item)
        display_text = item.display or item.label or item.name or tostring(item.value or "[Table Entry]")
      else
        item_value = item
        display_text = tostring(item)
      end
      return {
        value = item_value,
        display = display_text,
        filename = (type(item_value) == 'table' and (item_value.filename or item_value.file_path)) or (type(item) == 'string' and item or nil),
        lnum = (type(item_value) == 'table' and item_value.lnum),
        col = (type(item_value) == 'table' and item_value.col),
      }
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
    actions = {
      ["default"] = function(selected_list, fzf_opts_runtime)
        local display_key = selected_list and #selected_list > 0 and selected_list[1] or nil
        if not display_key then return end
        local item = display_to_processed_item[display_key]
        if not item then return end
        
        if spec.on_submit then
          vim.schedule(function() spec.on_submit(item.value) end)
          return
        end

        if item.filename and type(item.filename) == "string" then
          local location_str = item.filename
          if item.lnum then location_str = location_str .. ":" .. item.lnum end
          if item.col then location_str = location_str .. ":" .. item.col end
          fzf_actions.resume_term()
          fzf_actions.file_edit({ location_str }, fzf_opts_runtime)
        end
      end,
      ["ctrl-c"] = function()
        if spec.on_cancel then
          vim.schedule(function() spec.on_cancel() end)
        end
      end,
    },
  }

  -- ★★★ あなたのアプローチと逆引きマップを融合した、正しいプレビューワーの実装 ★★★
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
          line = item.lnum,
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
