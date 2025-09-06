-- lua/UNL/Buf/debug_log.lua (新設)

local log_engine = require("UNL.backend.buf.log")
local M = {}

local handle -- シングルトンなウィンドウハンドル

local function ensure_handle()
  if not handle then
    handle = log_engine.create({
      id = "unl_debug_log",
      title = "[[ UNL DEBUG LOG ]]",
      filetype = "unl-debug",
      auto_scroll = true,
      positioning = {
        strategy = 'primary',
        location = 'right',
        size = 0.4, -- 画面右側に40%の幅で表示
      },
      keymaps = {
        ["q"] = "<cmd>lua require('UNL.Buf.debug_log').close()<cr>",
      },
    })
  end
end

function M.open()
  ensure_handle()
  handle:open()
end

function M.close()
  if handle then handle:close() end
end

function M.toggle()
  if M.is_open() then M.close() else M.open() end
end

function M.is_open()
  return handle and handle:is_open()
end

function M.add_line(line)
  if M.is_open() then
    handle:add_lines({ line })
  end
end
    
return M
