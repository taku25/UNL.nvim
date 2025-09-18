-- lua/UNL/backend/grep_picker/provider/fzf_lua.lua

local M = { name = "fzf-lua" }

function M.available()
  -- このプロバイダーはripgrep(rg)が必須であることを明記
  return pcall(require, "fzf-lua") and vim.fn.executable("rg") == 1
end

function M.run(spec)
  spec = spec or {}
  
  local fzf_lua = require("fzf-lua")
  local log = require("UNL.logging").get(spec.logger_name or "UNL")
  
  if not spec.search_paths or #spec.search_paths == 0 then
    log.warn("fzf-lua: No search_paths provided for grep.")
    return
  end

  -- ★ 1. `rg`に渡すオプションを「テーブル」として組み立てる
  -- これにより、シェルの解釈を100%バイパスできる
  local rg_opts_table = {
    "--vimgrep",
    "--line-number",
    "--column",
    "--smart-case",
    "--no-heading",
    "--hidden",
  }

  -- Excludes (ディレクトリ除外)
  local excludes = spec.exclude_directories or {}
  for _, dir in ipairs(excludes) do
    table.insert(rg_opts_table, "--glob"); table.insert(rg_opts_table, "!" .. dir)
  end

  -- Includes (拡張子指定)
  local extensions = spec.include_extensions or {}
  if #extensions > 0 then
    for _, ext in ipairs(extensions) do
      table.insert(rg_opts_table, "-g"); table.insert(rg_opts_table, "*." .. ext)
    end
  end
  
  -- ★ 2. `live_grep_native`ではなく、高レベルな`live_grep`を呼び出す
  fzf_lua.live_grep({
    prompt = spec.title or "Live Grep> ",
    
    -- ★ 3. 組み立てたオプションを、正しいオプションキーに渡す
    search_dirs = spec.search_paths,
    rg_opts = table.concat(rg_opts_table, " "), -- fzf-luaはこの形式を期待している

    actions = {
      ["default"] = function(selected)
        local entry = selected[1]
        if not entry then return end
        
        local file, lnum, col = entry:match("^([^:]+):(%d+):(%d+):.*$")
        
        if file and lnum and spec.on_submit then
          pcall(spec.on_submit, { filename = file, lnum = tonumber(lnum), col = tonumber(col) })
        end
      end,
    },
  })
end

return M
