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

-- ★修正: バッファが存在すれば、ウィンドウが表示されていなくても書き込む
function LogHandle:add_lines(lines)
  -- バッファが無効なら何もしない
  if not self._buf or not vim.api.nvim_buf_is_valid(self._buf) then return end

  -- ウィンドウが表示されている場合のみ、自動スクロール判定を行う
  local should_autoscroll = self._spec.auto_scroll ~= false
  local is_at_bottom = false
  local win_valid = self:is_open()

  if win_valid and should_autoscroll then
    local total_lines = vim.api.nvim_buf_line_count(self._buf)
    local cursor_line = vim.api.nvim_win_get_cursor(self._win_id)[1]
    if cursor_line >= total_lines then is_at_bottom = true end
  end

  vim.api.nvim_set_option_value("modifiable", true, { buf = self._buf })
  vim.api.nvim_buf_set_lines(self._buf, -1, -1, false, lines)
  vim.api.nvim_set_option_value("modifiable", false, { buf = self._buf })

  -- ウィンドウが表示されていて、かつ底にいたならスクロール
  if win_valid and is_at_bottom then
    vim.api.nvim_win_set_cursor(self._win_id, { vim.api.nvim_buf_line_count(self._buf), 0 })
  end
end

function LogHandle:close()
  if self:is_open() then 
    pcall(vim.api.nvim_win_close, self._win_id, true)
  end
  self._win_id = nil
  -- バッファは削除しない (再利用のため)
  
  if self._children then
      for child_id, _ in pairs(self._children) do
      local child_handle = M.get_handle(child_id)
      if child_handle then child_handle:close() end
    end
  end
  _windows[self._spec.id] = nil
end

-- ★新規: バッファだけを先に作成・設定するメソッド
function LogHandle:setup_buffer()
  if self._buf and vim.api.nvim_buf_is_valid(self._buf) then
    return self._buf
  end

  self._buf = vim.api.nvim_create_buf(false, true)
  vim.bo[self._buf].buftype = "nofile"
  vim.bo[self._buf].swapfile = false
  -- ★重要: バッファを隠しても消えないようにする
  vim.bo[self._buf].bufhidden = "hide" 
  vim.bo[self._buf].filetype = self._spec.filetype or "log"
  
  -- キーマップ設定
  if self._spec.keymaps then
    for key, func_str in pairs(self._spec.keymaps) do
      vim.api.nvim_buf_set_keymap(self._buf, "n", key, func_str, { noremap = true, silent = true })
    end
  end
  
  return self._buf
end

-- ★新規: 既存のウィンドウにこのバッファをアタッチする
function LogHandle:attach_to_win(win_id)
  if not win_id or not vim.api.nvim_win_is_valid(win_id) then return end
  
  self:setup_buffer() -- バッファが無ければ作る
  self._win_id = win_id
  vim.api.nvim_win_set_buf(win_id, self._buf)
  vim.api.nvim_set_option_value("statusline", self._spec.title or "", { win = win_id })
  
  _windows[self._spec.id] = self
end

-- 既存の open は後方互換のために残すが、内部実装を変更
function LogHandle:open()
  if self:is_open() then return end

  -- 設定から位置とサイズを計算
  local pos = self._spec.positioning or {}
  local location = pos.location or "right"
  local size_ratio = pos.size or 0.5
  
  local cmd = "botright vertical 40new"
  local cols = vim.o.columns
  local lines = vim.o.lines

  if location == "right" then
    cmd = "botright vertical " .. math.floor(cols * size_ratio) .. "new"
  elseif location == "left" then
    cmd = "topleft vertical " .. math.floor(cols * size_ratio) .. "new"
  elseif location == "bottom" then
    cmd = "botright " .. math.floor(lines * size_ratio) .. "new"
  elseif location == "top" then
    cmd = "topleft " .. math.floor(lines * size_ratio) .. "new"
  end

  vim.cmd(cmd)
  local win_id = vim.api.nvim_get_current_win()
  self:attach_to_win(win_id)
end

function M.batch_open(handles, layout_cmd, on_all_opened)
  vim.schedule(function()
    vim.cmd(layout_cmd)
    local win = vim.api.nvim_get_current_win()
    -- batch_open は今回ULGでは使わなくなるが、他への影響を最小限にするため
    -- 最初のハンドルだけアタッチしておく実装にする
    if handles[1] then
       handles[1]:attach_to_win(win)
    end
    if on_all_opened then pcall(on_all_opened, handles) end
  end)
end

function M.create(spec)
  local self = {
    _spec = spec,
    _win_id = nil,
    _buf = nil,
    _children = {}, 
  }
  return setmetatable(self, LogHandle)
end

function M.get_handle(id) return _windows[id] end

return M
