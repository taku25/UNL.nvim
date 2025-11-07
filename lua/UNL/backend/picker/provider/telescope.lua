-- lua/UNL/backend/picker/provider/telescope.lua

local M = { name = "telescope" }

function M.available()
  return pcall(require, "telescope")
end

function M.run(spec)
  spec = spec or {}
  
  -- 必要なTelescopeモジュールを読み込む
  local actions = require("telescope.actions")
  local action_state = require("telescope.actions.state")
  local previewers = require("telescope.previewers")
  local finders = require("telescope.finders")
  local pickers = require("telescope.pickers")
  local sorters = require("telescope.sorters")
  local log = require("UNL.logging").get(spec.logger_name or "UNL")

  -- ★★★ DEV ICONS SUPPORT ★★★
  -- 1. 必要なモジュールを安全に読み込む
  local entry_display = require("telescope.pickers.entry_display")
  local devicons_ok, devicons = pcall(require, "nvim-web-devicons")

  -- 2. deviconを有効にするかどうかのフラグを決定
  local use_devicons = spec.devicons_enabled and devicons_ok

  -- 3. deviconが有効な場合、カスタムdisplayerを作成
  local displayer, make_display
  if use_devicons then
    displayer = entry_display.create({
      separator = " ",
      items = {
        { width = 2 },         -- アイコン用のスペース
        { remaining = true },  -- 残りのテキスト用のスペース
      },
    })

    make_display = function(entry)
      -- entry_makerから渡された情報を使って、表示チャンクを作成
      return displayer({
        { entry.icon, entry.icon_hl }, -- { "アイコン文字", "ハイライトグループ" }
        entry.display_text,
      })
    end
  end
  -- ★★★ DEV ICONS SUPPORTここまで ★★★

  local finder
  if spec.items then
    finder = finders.new_table({
      results = spec.items,
      entry_maker = function(entry)
        -- STEP A: 従来のロジックで、value, display, filenameを特定
        local value, display, filename
        if type(entry) == 'table' then
          value = entry.value or entry
          display = entry.display or entry.label or entry.name or tostring(value)
          filename = entry.filename or (type(value) == 'table' and value.filename)
        else
          value = entry
          display = tostring(entry)
          filename = tostring(entry) -- 文字列の場合は、それ自体がファイル名であると仮定
        end
        if type(value) == 'table' and string.match(display, "^table: 0x") then
          display = value.display or value.label or value.name or display
        end

        local result = {
          value = value,
          display = display,
          ordinal = display,
          filename = filename,
        }
        
        -- ★★★ DEV ICONS SUPPORT ★★★
        -- STEP B: deviconが有効な場合、アイコン情報をresultテーブルに追加
        if use_devicons and filename and type(filename) == 'string' then
          local extension = vim.fn.fnamemodify(filename, ":e")
          local icon, icon_hl = devicons.get_icon(filename, extension)
          
          result.display = make_display -- 表示関数を上書き
          result.icon = icon or ""
          result.icon_hl = icon_hl or "Normal"
          result.display_text = display -- 元の表示テキストを保持
        end
        -- ★★★ DEV ICONS SUPPORTここまで ★★★

        return result
      end,
    })
  else
    log.warn("Telescope provider (static): No 'items' provided.")
    return
  end

  local picker_opts = {
    prompt_title = spec.title or "Select Item",

    file_ignore_patterns = {},
    find_files_ignore_patterns = {},
    hidden = true ,
    finder = finder,
    sorter = sorters.get_generic_fuzzy_sorter({}),
    cwd = spec.cwd or vim.loop.cwd(),
    attach_mappings = function(prompt_bufnr, map)
      -- 以前の複雑なon_submitロジックを、よりシンプルで堅牢なものに修正
      actions.select_default:replace(function()
        local picker = action_state.get_current_picker(prompt_bufnr)
        actions.close(prompt_bufnr)
        
        -- entry.valueに常に正しいデータが入っているので、それを使うだけ
        local get_value = function(entry) return entry and entry.value or nil end

        if spec.multi_select then
          local results = {}
          for _, entry in ipairs(picker:get_multi_selection()) do
            table.insert(results, get_value(entry))
          end
          -- 単一選択がマルチ選択にフォールバックした場合も考慮
          if #results == 0 then
            local single_entry = action_state.get_selected_entry()
            if single_entry then table.insert(results, get_value(single_entry)) end
          end
          if spec.on_submit then vim.schedule(function() spec.on_submit(results) end) end
        else
          local selection = action_state.get_selected_entry()
          if spec.on_submit then vim.schedule(function() spec.on_submit(get_value(selection)) end) end
        end
      end)
      return true
    end,
  }

  -- ★★★ DEV ICONS SUPPORT ★★★
  -- deviconが有効な場合、pickerにentry_displayを設定
  if use_devicons then
    picker_opts.entry_display = make_display
  end
  -- ★★★ DEV ICONS SUPPORTここまで ★★★

  if spec.preview_enabled ~= false then 
    picker_opts.previewer = previewers.vim_buffer_cat.new({ title = "Preview" }) 
  end
  
  pickers.new(picker_opts):find()
end

return M
