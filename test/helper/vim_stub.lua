-- 最低限の vim グローバルをテスト用に用意
if _G.vim == nil then
  local notify_calls = {}

  local next_id = 100
  local function newid()
    next_id = next_id + 1
    return next_id
  end

  local wins_valid = {}
  local bufs = {}

  _G.vim = {
    notify = function(msg, level, opts)
      notify_calls[#notify_calls+1] = { msg = msg, level = level, opts = opts }
    end,
    log = { levels = { INFO = 1, ERROR = 2, WARN = 3 } },
    loop = {
      hrtime = function()
        return math.floor(os.clock() * 1e9)
      end,
    },
    defer_fn = function(fn, _ms)
      -- テストでは即時
      pcall(fn)
    end,
    api = {
      nvim_list_uis = function()
        return { { width = 160, height = 50 } }
      end,
      nvim_create_buf = function(_, _)
        local id = newid()
        bufs[id] = {}
        return id
      end,
      nvim_open_win = function(buf, _, _)
        local win = newid()
        wins_valid[win] = true
        return win
      end,
      nvim_win_is_valid = function(win)
        return wins_valid[win] and true or false
      end,
      nvim_buf_set_lines = function(_buf, _, _, _, _lines) end,
      nvim_set_option_value = function() end,
      nvim_win_close = function(win, _force)
        wins_valid[win] = nil
      end,
    },
  }

  _G.__TEST_VIM_NOTIFY_POP = function()
    local out = notify_calls
    notify_calls = {}
    return out
  end
end

return true
