-- lua/UNL/backend/picker/provider/fzf_lua.lua

local M = { name = "fzf-lua" }

function M.available()
  return pcall(require, "fzf-lua")
end

function M.run(spec)
  local fzf_lua = require("fzf-lua")
  local log = require("UNL.logging").get("UNL")
  spec = spec or {}

  local fzf_opts = {
    prompt = spec.title or "Select Item> ",
    cwd = spec.cwd or vim.loop.cwd(),
    actions = {
      ["default"] = function(selected)
        if spec.on_submit then
          local result = selected and #selected > 0 and selected[1] or nil
          if result and spec.items and type(spec.items[1]) == "table" then
            for _, item in ipairs(spec.items) do
              if item.label == result then
                result = item.value
                break
              end
            end
          end
          vim.schedule(function() spec.on_submit(result) end)
        end
      end,
      ["ctrl-c"] = function()
        if spec.on_cancel then
          vim.schedule(function() spec.on_cancel() end)
        end
      end,
    },
  }

  if spec.items then
    -- Case 1: items (テーブル) が渡された場合
    local display_items = {}
    for _, item in ipairs(spec.items) do
      table.insert(display_items, item.label or tostring(item))
    end
    fzf_lua.fzf_exec(display_items, fzf_opts)

  elseif spec.exec_cmd then
    -- Case 2: exec_cmd (直接コマンド) が渡された場合
    local cmd_string = type(spec.exec_cmd) == "table" and table.concat(spec.exec_cmd, " ") or spec.exec_cmd
    fzf_lua.fzf_exec(cmd_string, fzf_opts)
    
  else
    log.warn("fzf-lua provider: No items or exec_cmd provided.")
    return
  end
end

return M
