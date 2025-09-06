

local M = {}
local _windows = {}

function M.create(spec)
  local self = {
    _spec = spec,
    _win_id = nil,
    _buf = nil,
  }

  -- is_open, get_win_id, add_lines, close の各関数は変更ありません...
  function self:is_open()
    return self._win_id and vim.api.nvim_win_is_valid(self._win_id)
  end
  
  function self:get_win_id()
    return self:is_open() and self._win_id or nil
  end

  function self:add_lines(lines)
    if not self:is_open() then return end
    local should_autoscroll = self._spec.auto_scroll ~= false
    local is_at_bottom = false
    if should_autoscroll then
      local total_lines = vim.api.nvim_buf_line_count(self._buf)
      local cursor_line = vim.api.nvim_win_get_cursor(self._win_id)[1]
      if cursor_line >= total_lines then
        is_at_bottom = true
      end
    end
    vim.api.nvim_set_option_value("modifiable", true, { buf = self._buf })
    vim.api.nvim_buf_set_lines(self._buf, -1, -1, false, lines)
    vim.api.nvim_set_option_value("modifiable", false, { buf = self._buf })
    if is_at_bottom then
      vim.api.nvim_win_set_cursor(self._win_id, { vim.api.nvim_buf_line_count(self._buf), 0 })
    end
  end

  function self:close()
    if not self:is_open() then return end
    vim.api.nvim_win_close(self._win_id, true)
    _windows[self._spec.id] = nil
    self._win_id = nil
  end


  function self:open()
    if self:is_open() then
      vim.api.nvim_set_current_win(self._win_id)
      return
    end

    self._buf = vim.api.nvim_create_buf(false, true)
    vim.bo[self._buf].buftype = "nofile"; vim.bo[self._buf].swapfile = false
    vim.bo[self._buf].filetype = self._spec.filetype or "log"

    local pos_spec = self._spec.positioning or {}
    local strategy = pos_spec.strategy or 'primary'
    local win_open_cmd = ""
    -- (ウィンドウ分割ロジックは変更なし)
    if strategy == 'secondary' and pos_spec.base_id then
      local base_handle = M.get_handle(pos_spec.base_id)
      if base_handle and base_handle:is_open() then
        vim.api.nvim_set_current_win(base_handle:get_win_id())
        local ratio = pos_spec.split_size or 0.3
        local loc = pos_spec.split_location or 'bottom_of'
        local size
        if loc == 'bottom_of' or loc == 'top_of' then
          local base_height = vim.api.nvim_win_get_height(base_handle:get_win_id())
          size = math.floor(base_height * ratio)
          win_open_cmd = loc == 'bottom_of' and ("botright " .. size .. " split new") or ("topleft " .. size .. " split new")
        else
          local base_width = vim.api.nvim_win_get_width(base_handle:get_win_id())
          size = math.floor(base_width * ratio)
          win_open_cmd = loc == 'right_of' and ("botright " .. size .. " vsplit new") or ("topleft " .. size .. " vsplit new")
        end
      else
        strategy = 'primary'
      end
    end
    if strategy == 'primary' then
      local ratio = pos_spec.size or 0.2
      local loc = pos_spec.location or 'bottom'
      local size
      if loc == 'bottom' or loc == 'top' then
        size = math.floor(vim.o.lines * ratio)
        win_open_cmd = loc == 'bottom' and ("botright " .. size .. " new") or ("topleft " .. size .. " new")
      elseif loc == 'left' or loc == 'right' then
        size = math.floor(vim.o.columns * ratio)
        win_open_cmd = loc == 'left' and ("vertical topleft " .. size .. " new") or ("vertical botright " .. size .. " new")
      elseif loc == 'tab' then win_open_cmd = "tabnew" end
    end
    
    if win_open_cmd and win_open_cmd ~= "" then
      vim.cmd(win_open_cmd)
    else
      -- ★★★ `log.error` の代わりに、安全な `vim.notify` を使用します ★★★
      vim.notify("UNL.Buf.log: Could not determine window open command for spec: " .. vim.inspect(self._spec), vim.log.levels.ERROR)
      return
    end
    
    local current_win_id = vim.api.nvim_get_current_win()
    self._win_id = current_win_id
    vim.api.nvim_win_set_buf(self._win_id, self._buf)
    vim.api.nvim_set_option_value("statusline", self._spec.title or "", { win = self._win_id })

    if self._spec.keymaps then
      for key, func_str in pairs(self._spec.keymaps) do
        vim.api.nvim_buf_set_keymap(self._buf, "n", key, func_str, { noremap = true, silent = true })
      end
    end
    _windows[self._spec.id] = self
  end
  return self
end

function M.get_handle(id)
  return _windows[id]
end

return M
