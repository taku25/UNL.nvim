-- lua/UNL/backend/dynamic_stack_picker/provider/telescope.lua

local M = { name = "telescope" }

function M.available()
  return pcall(require, "telescope")
end

function M.run(spec)
  spec = spec or {}
  
  local actions = require("telescope.actions")
  local action_state = require("telescope.actions.state")
  local finders = require("telescope.finders")
  local pickers = require("telescope.pickers")
  local conf = require("telescope.config").values
  local log = require("UNL.logging").get(spec.logger_name or "UNL")

  local devicons_ok, devicons = pcall(require, "nvim-web-devicons")
  local use_devicons = spec.devicons_enabled ~= false and devicons_ok

  -- 内部で保持するリザルトリスト
  local results = {}
  
  -- エントリ作成関数
  local make_entry = function(entry)
    local value, display, filename, lnum, col
    
    if type(entry) == 'table' then
      value = entry.value or entry
      display = entry.display or entry.label or entry.name or tostring(value)
      filename = entry.filename or (type(value) == 'table' and value.filename)
      lnum = entry.lnum or entry.line or entry.row
      col = entry.col
    else
      value = entry
      display = tostring(entry)
      filename = tostring(entry)
    end

    local result = {
      value = value,
      display = display,
      ordinal = display,
      filename = filename,
      lnum = lnum and tonumber(lnum),
      col = col and tonumber(col),
    }
    
    if use_devicons and filename and type(filename) == 'string' then
      local extension = vim.fn.fnamemodify(filename, ":e")
      local icon, icon_hl = devicons.get_icon(filename, extension)
      result.icon = icon or ""
      result.icon_hl = icon_hl or "Normal"
    end

    return result
  end

  -- カスタムFinder: 呼び出されるたびに現在の results をすべて流し込む
  local finder = setmetatable({
    results = results,
    close = function() end
  }, {
    __call = function(_, _, cb, cb_complete)
      for _, item in ipairs(results) do
        local entry = make_entry(item)
        if entry then cb(entry) end
      end
      if cb_complete then cb_complete() end
    end
  })

  local picker_opts = {
    prompt_title = spec.title or "Stacking Items...",
    finder = finder,
    sorter = conf.generic_sorter({}),
    sorting_strategy = "ascending",
    attach_mappings = function(prompt_bufnr, map)
      actions.select_default:replace(function()
        local selection = action_state.get_selected_entry()
        actions.close(prompt_bufnr)
        if selection and spec.on_submit then
          vim.schedule(function() spec.on_submit(selection.value) end)
        end
      end)
      return true
    end,
  }

  local picker = pickers.new(picker_opts)
  -- 内部エラー防止のため tiebreak をセット
  picker.tiebreak = function() return false end
  picker:find()

  -- push関数の定義
  local push = function(items)
    if not items then return end
    
    local to_add = {}
    if type(items) == "table" and items[1] ~= nil then
      for _, item in ipairs(items) do table.insert(to_add, item) end
    else
      table.insert(to_add, items)
    end

    -- データを蓄積
    for _, item in ipairs(to_add) do
      table.insert(results, item)
    end
    
    -- UIの更新を依頼
    vim.schedule(function()
      if picker.prompt_bufnr and vim.api.nvim_buf_is_valid(picker.prompt_bufnr) then
        -- _on_lines() は Telescope が入力バッファの変更を検知した際に呼ぶ内部関数。
        -- これを明示的に呼ぶことで、Telescope に「再検索（＝Finderの再実行）」を強制させ、
        -- 画面を最新の状態に更新する。
        if picker._on_lines then
          picker._on_lines()
        end
      end
    end)
  end

  -- 処理開始
  if spec.start then
    spec.start(push)
  end
end

return M