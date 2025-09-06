-- UNL.nvim/lua/UNL/backend/buf/log.lua (最後の部品交換を完了した最終版)

local M = {}
local _windows = {}

local LogHandle = {}
LogHandle.__index = LogHandle

function LogHandle:is_open()
  return self._win_id and vim.api.nvim_win_is_valid(self._win_id)
end
function LogHandle:get_win_id()
  return self:is_open() and self._win_id or nil
end
function LogHandle:add_child(child_id) self._children[child_id] = true end
function LogHandle:remove_child(child_id) self._children[child_id] = nil end
function LogHandle:add_lines(lines)
  if not self:is_open() then return end
  local should_autoscroll = self._spec.auto_scroll ~= false
  local is_at_bottom = false
  if should_autoscroll then
    local total_lines = vim.api.nvim_buf_line_count(self._buf)
    local cursor_line = vim.api.nvim_win_get_cursor(self._win_id)[1]
    if cursor_line >= total_lines then is_at_bottom = true end
  end
  vim.api.nvim_set_option_value("modifiable", true, { buf = self._buf })
  vim.api.nvim_buf_set_lines(self._buf, -1, -1, false, lines)
  vim.api.nvim_set_option_value("modifiable", false, { buf = self._buf })
  if is_at_bottom then
    vim.api.nvim_win_set_cursor(self._win_id, { vim.api.nvim_buf_line_count(self._buf), 0 })
  end
end

function LogHandle:close()
  if not self:is_open() then return end
  
  -- ULGでは司令官が直接閉じるが、汎用エンジンのためロジックは残しておく
  if self._children then
      for child_id, _ in pairs(self._children) do
      local child_handle = M.get_handle(child_id)
      if child_handle and child_handle:is_open() then child_handle:close() end
    end
  end

  if #vim.api.nvim_list_wins() == 1 and vim.api.nvim_get_current_win() == self._win_id then
    vim.cmd("quit")
  else
    vim.api.nvim_win_close(self._win_id, true)
  end

  _windows[self._spec.id] = nil
  self._win_id = nil
  local pos_spec = self._spec.positioning or {}
  if pos_spec.strategy == 'secondary' and pos_spec.base_id then
    local parent_handle = M.get_handle(pos_spec.base_id)
    if parent_handle then parent_handle:remove_child(self._spec.id) end
  end
end

function M.batch_open(handles, layout_cmd, on_all_opened)
  vim.schedule(function()
    local wins_before = {}
    for _, win in ipairs(vim.api.nvim_list_wins()) do wins_before[win] = true end
    vim.cmd(layout_cmd)
    local new_wins = {}
    for _, win in ipairs(vim.api.nvim_list_wins()) do
      if not wins_before[win] then table.insert(new_wins, win) end
    end
    if #new_wins == #handles then
      for i, handle in ipairs(handles) do
        handle:_attach_to_window(new_wins[i])
      end
      -- (サイズ調整ロジックはあなたの既存コードのまま)
      if on_all_opened then pcall(on_all_opened, handles) end
    else
      vim.notify("ULG Error: Window creation mismatch.", vim.log.levels.ERROR)
    end
  end)
end

function LogHandle:_attach_to_window(win_id)
  self._win_id = win_id
  self._buf = vim.api.nvim_create_buf(false, true)
  _windows[self._spec.id] = self
  vim.api.nvim_win_set_buf(self._win_id, self._buf)
  vim.bo[self._buf].buftype = "nofile"; vim.bo[self._buf].swapfile = false
  vim.bo[self._buf].filetype = self._spec.filetype or "log"
  vim.api.nvim_set_option_value("statusline", self._spec.title or "", { win = self._win_id })
  if self._spec.keymaps then
    for key, func_str in pairs(self._spec.keymaps) do
      vim.api.nvim_buf_set_keymap(self._buf, "n", key, func_str, { noremap = true, silent = true })
    end
  end
end


-- ★★★ ここが最後の修正箇所です ★★★
function M.create(spec)
  local self = {
    _spec = spec,
    _win_id = nil,
    _buf = nil,
    _children = {}, -- ← _children を空テーブルとして必ず初期化する
  }
  return setmetatable(self, LogHandle)
end

function M.get_handle(id) return _windows[id] end

return M
