-- lua/UNL/backend/dynamic_picker/provider/telescope.lua (with highlights)

local M = { name = "telescope" }

function M.available()
  return pcall(require, "telescope.pickers")
end

function M.run(spec)
  spec = spec or {}
  assert(spec.command, "Telescope dynamic_picker requires 'spec.command'")

  local pickers = require("telescope.pickers")
  local finders = require("telescope.finders")
  local actions = require("telescope.actions")
  local action_state = require("telescope.actions.state")
  local conf = require("telescope.config").values
  local previewers = require("telescope.previewers")
  -- ▼▼▼【変更点】entry_display を require します ▼▼▼
  local entry_display = require("telescope.pickers.entry_display")
  -- ▲▲▲ 変更ここまで ▲▲▲
  local devicons_ok, devicons = pcall(require, "nvim-web-devicons")

  local cmd_table = { spec.command }
  if spec.args and type(spec.args) == "table" then
    vim.list_extend(cmd_table, spec.args)
  end

  -- ▼▼▼【変更点】カスタムの entry_displayer を作成します ▼▼▼
  local displayer = entry_display.create({
    separator = " ",
    items = {
      { width = 2 }, -- アイコンのためのスペース
      { remaining = true }, -- 残りのテキストのためのスペース
    },
  })

  local make_display = function(entry)
    -- entry_maker から渡された情報を使って、表示チャンクを作成します
    return displayer({
      { entry.icon, entry.icon_hl }, -- { "アイコン文字", "ハイライトグループ名" }
      entry.display_text,
    })
  end
  -- ▲▲▲ 変更ここまで ▲▲▲

  local picker_opts = {
    prompt_title = spec.title or "Dynamic Items",
    finder = finders.new_oneshot_job(cmd_table, {
      cwd = spec.cwd,
      entry_maker = function(line)
        local parts = vim.split(line, "\t", { plain = true, trimempty = true })
        local display_text = parts[1]
        local file_path = parts[2]

        if not (display_text and file_path) then
          return { display = line, value = line, ordinal = line }
        end

        -- ▼▼▼【変更点】アイコンとそのハイライトグループを取得します ▼▼▼
        local icon, icon_hl
        if devicons_ok and spec.devicons_enabled ~= false then
          local extension = vim.fn.fnamemodify(file_path, ":e")
          -- get_icon はアイコン文字とハイライトグループ名の2つを返します
          icon, icon_hl = devicons.get_icon(file_path, extension)
        end
        -- ▲▲▲ 変更ここまで ▲▲▲

        -- ▼▼▼【変更点】displayerで利用するデータを返します ▼▼▼
        return {
          -- display は関数である必要があります
          display = make_display,
          -- displayerで使うためのカスタムフィールド
          icon = icon or "",
          icon_hl = icon_hl or "Normal",
          display_text = display_text,
          -- 元のフィールドも維持します
          value = file_path,
          ordinal = display_text,
          filename = file_path,
        }
        -- ▲▲▲ 変更ここまで ▲▲▲
      end,
    }),
    sorter = conf.generic_sorter({}),
    -- ▼▼▼【変更点】作成した entry_displayer をピッカーに設定します ▼▼▼
    entry_display = make_display,
    -- ▲▲▲ 変更ここまで ▲▲▲
    attach_mappings = function(prompt_bufnr, map)
      actions.select_default:replace(function()
        actions.close(prompt_bufnr)
        local selection = action_state.get_selected_entry()
        if spec.on_submit then
          vim.schedule(function()
            spec.on_submit(selection and selection.value or nil)
          end)
        end
      end)
      return true
    end,
  }

  if spec.preview_enabled ~= false then
    picker_opts.previewer = conf.file_previewer({})
  end

  pickers.new(picker_opts):find()
end

return M
