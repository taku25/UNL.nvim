-- lua/UNL/buf/open.lua
-- A generic helper module for safe window operations.

local log = require("UNL.logging")

local M = {}

---
-- Checks if a given buffer is "safe" to open a new file in, based on config.
-- @param bufnr integer: The buffer number to check.
-- @param conf table: The 'safe_open' configuration table.
-- @return boolean: True if the buffer is safe, false otherwise.
local function is_safe_buffer(bufnr, conf)
  if not vim.api.nvim_buf_is_valid(bufnr) then return false end
  if not vim.api.nvim_buf_get_option(bufnr, "modifiable") then return false end

  local buftype = vim.api.nvim_buf_get_option(bufnr, "buftype")
  if vim.tbl_contains(conf.prevent_in_buftypes, buftype) then
    return false
  end

  local filetype = vim.api.nvim_buf_get_option(bufnr, "filetype")
  if vim.tbl_contains(conf.prevent_in_filetypes, filetype) then
    return false
  end

  return true
end

---
-- Finds the first "safe" window that is suitable for opening a file.
-- @param conf table: The 'safe_open' configuration table.
-- @return integer|nil: The window ID if a safe one is found, otherwise nil.
local function find_safe_window(conf)
  for _, win_id in ipairs(vim.api.nvim_list_wins()) do
    local bufnr = vim.api.nvim_win_get_buf(win_id)
    if is_safe_buffer(bufnr, conf) then
      return win_id
    end
  end
  return nil
end

---
-- Opens a file in a "safe" window.
-- @param opts table: Options for opening the file.
--   - file_path (string): The absolute path of the file to open.
--   - open_cmd (string): The command to use ("edit", "split", "vsplit").
--   - plugin_name (string): The name of the calling plugin (e.g., "UCM").
--   - split_cmd (string, optional): The command to use when creating a new window (default: "vsplit").
function M.safe(opts)
  opts = opts or {}
  if not (opts.file_path and opts.open_cmd and opts.plugin_name) then
    log.get("UNL").error("safe_open.open requires file_path, open_cmd, and plugin_name.")
    return
  end
  
  local conf_all = require("UNL.config").get(opts.plugin_name)
  local conf_safe_open = conf_all.safe_open
  local logger = log.get(opts.plugin_name)

  if not conf_safe_open then
    logger.warn("safe_open config not found. Opening with standard command.")
    vim.cmd(opts.open_cmd .. " " .. vim.fn.fnameescape(opts.file_path))
    return
  end

  local current_win = vim.api.nvim_get_current_win()
  local current_buf = vim.api.nvim_win_get_buf(current_win)
  local target_win_id

  if is_safe_buffer(current_buf, conf_safe_open) then
    target_win_id = current_win
  else
    target_win_id = find_safe_window(conf_safe_open)
  end

  if target_win_id then
    vim.api.nvim_set_current_win(target_win_id)
    vim.cmd(opts.open_cmd .. " " .. vim.fn.fnameescape(opts.file_path))
  else
    -- 安全なウィンドウが見つからない場合は新規分割
    local split_cmd = opts.split_cmd or "vsplit"
    logger.debug("No safe window found. Creating a new window with: " .. split_cmd)
    vim.cmd(split_cmd .. " " .. vim.fn.fnameescape(opts.file_path))
  end
end

return M
