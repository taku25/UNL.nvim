local M = { name = "telescope" }

function M.available()
  return pcall(require, "telescope")
end

function M.run(spec)
  local telescope = require("telescope")

  local builtint = require('telescope.builtin')
  local actions = require("telescope.actions")
  local action_state = require("telescope.actions.state")
  local log = require("UNL.logging").get(spec.logger_name or "UNL")
  if not M.available() then
    log.error("Telescope is not available for live grep.")
    return
  end

  spec = spec or {}
  if not spec.search_paths or #spec.search_paths == 0 then
    log.warn("Telescope: No search_paths provided for grep.")
    return
  end

  -- 1. rgコマンドに渡す「追加の」引数を組み立てる
  --    fzf-lua版と全く同じロジック
  local additional_args_parts = {}

  local conf = spec.conf or {}
  local grep_conf = conf.uep or conf
  local excludes = grep_conf.excludes_directory or {}
  for _, dir in ipairs(excludes) do
    table.insert(additional_args_parts, "--glob")
    table.insert(additional_args_parts, "!" .. dir)
  end

  local extensions = grep_conf.files_extensions or {}
  if #extensions > 0 then
    local extension_glob = "*." .. "{" .. table.concat(extensions, ",") .. "}"
    table.insert(additional_args_parts, "--glob")
    table.insert(additional_args_parts, extension_glob)
  end

  log.trace("Telescope live_grep additional_args: %s", table.concat(additional_args_parts, " "))
  log.trace("Telescope live_grep search_dirs: %s", vim.inspect(spec.search_paths))

  -- 2. Telescopeのlive_grepを、カスタムオプション付きで呼び出す
  builtint.live_grep({
    -- ★★★ ここが核心 ★★★
    prompt_title = spec.title or "Live Grep",

    -- 検索対象のディレクトリを、UEPが解決したパスのリストで指定
    search_dirs = spec.search_paths,
    
    -- rgに渡す追加の引数（globパターンなど）
    additional_args = additional_args_parts,
    
    -- ★★★ 選択時の挙動をカスタマイズ ★★★
    attach_mappings = function(bufnr, map)
      actions.select_default:replace(function()
        -- ピッカーを閉じる
        actions.close(bufnr)
        
        -- 選択されたエントリー情報を取得
        local entry = action_state.get_selected_entry()
        
        if not entry then
          log.warn("Telescope: No entry selected.")
          return
        end
        
        -- specで渡されたコールバック関数を呼び出す
        -- Telescopeのentryはパース済みのテーブルなので、文字列処理は不要
        if spec.on_submit then
          pcall(spec.on_submit, { filename = entry.filename, lnum = entry.lnum, col = entry.col })
        end
      end)
      return true
    end,
  })
end

return M
