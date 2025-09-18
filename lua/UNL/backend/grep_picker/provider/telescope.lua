-- lua/UNL/backend/grep_picker/provider/telescope.lua

local M = { name = "telescope" }

function M.available()
  return pcall(require, "telescope")
end

function M.run(spec)
  spec = spec or {}
  
  -- 1. 必要なTelescopeモジュールをすべて読み込む
  local builtin = require('telescope.builtin')
  local actions = require("telescope.actions")
  local action_state = require("telescope.actions.state")
  local log = require("UNL.logging").get(spec.logger_name or "UNL")
  local make_entry = require("telescope.make_entry")
  local entry_display = require("telescope.pickers.entry_display")
  
  -- 2. deviconサポートを安全に読み込む
  local devicons_ok, devicons = pcall(require, "nvim-web-devicons")
  
  if not spec.search_paths or #spec.search_paths == 0 then
    log.warn("Telescope: No search_paths provided for grep.")
    return
  end

  -- 3. deviconサポートを準備する
  local use_devicons = spec.devicons_enabled and devicons_ok
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
      return displayer({
        { entry.icon, entry.icon_hl },
        entry.display_text,
      })
    end
  end

  -- 4. ripgrepに渡す追加引数を組み立てる
  local additional_args_parts = {}
  local excludes = spec.exclude_directories or {}
  for _, dir in ipairs(excludes) do
    table.insert(additional_args_parts, "--glob"); table.insert(additional_args_parts, "!" .. dir)
  end
  local extensions = spec.include_extensions or {}
  if #extensions > 0 then
    for _, ext in ipairs(extensions) do
      table.insert(additional_args_parts, "-g"); table.insert(additional_args_parts, "*." .. ext)
    end
  end

  -- 5. live_grepに渡すメインのオプションテーブルを準備する
  local grep_opts = {
    prompt_title = spec.title or "Live Grep",
    search_dirs = spec.search_paths,
    additional_args = additional_args_parts,
    attach_mappings = function(bufnr, map)
      actions.select_default:replace(function()
        actions.close(bufnr)
        local entry = action_state.get_selected_entry()
        if not entry then log.warn("Telescope: No entry selected."); return end
        if spec.on_submit then
          pcall(spec.on_submit, { filename = entry.filename, lnum = entry.lnum, col = entry.col })
        end
      end)
      return true
    end,
  }

  -- 6. 表示をカスタマイズする必要がある場合、entry_makerを上書きする
  if spec.transform_display or use_devicons then
    local default_entry_maker = make_entry.gen_from_vimgrep(spec)

    grep_opts.entry_maker = function(line)
      -- A. まずTelescopeのデフォルト処理に任せる
      local entry = default_entry_maker(line)
      if not entry then return nil end
      
      -- B. 表示パスを組み立てる (transform関数があればそれを使う)
      local display_path = (spec.transform_display and spec.transform_display(entry.filename)) or entry.filename
      local display_text = string.format("%s:%s:%s:%s", display_path, entry.lnum, entry.col, entry.text)
      
      -- C. deviconが有効なら、アイコン情報を追加し、displayを関数で上書き
      if use_devicons then
        local icon, icon_hl = devicons.get_icon(entry.filename, vim.fn.fnamemodify(entry.filename, ":e"))
        entry.display = make_display
        entry.icon = icon or ""
        entry.icon_hl = icon_hl or "Normal"
        entry.display_text = display_text
      else
      -- D. deviconが無効なら、組み立てた文字列をそのままdisplayに設定
        entry.display = display_text
      end
      
      return entry
    end
  end

  -- 7. deviconが有効な場合、pickerにカスタムentry_displayを設定
  if use_devicons then
    grep_opts.entry_display = make_display
  end
  
  -- 8. 最終的に完成したオプションでlive_grepを実行
  builtin.live_grep(grep_opts)
end

return M
