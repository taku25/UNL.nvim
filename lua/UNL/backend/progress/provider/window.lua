local Aggregator = require("UNL.backend.progress.aggregator")

local spec = {
  name = "window",
  category = "progress",
  weight = 50,
  capabilities = {
    window     = true,
    percentage = true,
    text_log   = true,
  },
  detect = function()
    return (type(vim) == "table") and vim.api ~= nil
  end,
  create = function(opts)
    if opts.enabled == false then return nil end
    local aggr = Aggregator.new(opts.weights)
    local max_lines = opts.window_progress_max_lines or 12
    local width     = opts.window_progress_width or 52
    local blend     = opts.window_progress_winblend or 10
    local title     = opts.title or "UEP Refresh"

    local ui = vim.api.nvim_list_uis()[1] or { width = width + 4 }
    local col = math.max(0, ui.width - width - 2)

    local buf = vim.api.nvim_create_buf(false, true)
    local win
    local lines = { title }

    local function ensure_win()
      if win and vim.api.nvim_win_is_valid(win) then return end
      local win_height = math.min(max_lines, 14)
      win = vim.api.nvim_open_win(buf, false, {
        relative = "editor",
        width = width,
        height = win_height,
        row = 1,
        col = col,
        style = "minimal",
        border = "rounded",
        noautocmd = true,
      })
      pcall(vim.api.nvim_set_option_value, "winblend", blend, { win = win })
      vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
    end

    local function push(msg)
      ensure_win()
      lines[#lines+1] = msg
      if #lines > max_lines then table.remove(lines, 1) end
      vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
    end

    local r = {}
    function r:define_from_plan(phases)
      aggr:define_from_plan(phases)
      push(string.format("Plan: %d phases", #phases))
    end
    function r:stage_define(name, total)
      aggr:define(name, total); push(aggr:format(name, 0, total))
    end
    function r:stage_update(name, done, total, msg)
      aggr:update(name, done, total); push(aggr:format(name, done, total))
    end
    function r:update(stage, message)
      push(aggr:format(stage, nil, nil))
    end
    function r:finish(success)
      local pct = aggr:percentage()
      push(success and string.format("Complete (%d%%)", pct) or "Failed")
      vim.defer_fn(function()
        if win and vim.api.nvim_win_is_valid(win) then
          pcall(vim.api.nvim_win_close, win, true)
        end
      end, 2000)
    end
    return r
  end,
}

return spec
