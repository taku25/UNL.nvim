-- lua/UNL/backend/grep_picker/provider/fzf_lua.lua (完成版)

local M = { name = "fzf-lua" }

function M.available()
  return pcall(require, "fzf-lua")
end

function M.run(spec)
  local fzf_lua = require("fzf-lua")
  local log = require("UNL.logging").get(spec.logger_name or "UNL")
  if not M.available() then
    log.error("fzf-lua is not available for live grep.")
    return
  end

  spec = spec or {}

  -- 1. rgコマンドに渡す「追加の」引数を組み立てる
  local additional_args_parts = {}

  -- 基本的なオプション
  table.insert(additional_args_parts, "--vimgrep")
  table.insert(additional_args_parts, "--line-number")
  table.insert(additional_args_parts, "--column")
  table.insert(additional_args_parts, "--smart-case")
  table.insert(additional_args_parts, "--no-heading")
  table.insert(additional_args_parts, "--hidden")

  -- Configから除外/追加 glob を設定
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

  -- 検索パスを追加
  -- rgの構文を確実にするため、オプションの終わりを示す -- を挟む
  table.insert(additional_args_parts, "--")
  for _, path in ipairs(spec.search_paths or {}) do
    table.insert(additional_args_parts, path)
  end

  -- 最終的な引数文字列を組み立てる
  local final_additional_args = table.concat(additional_args_parts, " ")

  log.trace("fzf-lua live_grep_native additional_args: %s", final_additional_args)

  fzf.live_grep_native({
    -- ★★★ あなたが発見した、真の解決策 ★★★
    additional_args = final_additional_args,
    prompt = spec.title or "Live Grep>",
    actions = {
      ["default"] = function(selected)
        local entry = selected[1]
        if not entry then return end
        local file, lnum = entry:match("^([^:]+):(%d+):.*$")
        if file and lnum and spec.on_submit then
          pcall(spec.on_submit, { filename = file, lnum = tonumber(lnum) })
        end
      end,
    },
  })
end

return M
