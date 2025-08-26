-- lua/UNL/backend/filer/provider/netrw.lua
-- Neovim標準のファイラー (netrw) を操作するためのプロバイダー

local M = { name = "native" }

---
-- netrwが利用可能かチェックする
-- netrwは標準搭載だが、ユーザーが無効化している場合がある
function M.available()
  -- g:loaded_netrw が 1 に設定されていると、netrw は無効化されている
  return not vim.g.loaded_netrw
end

---
-- netrwで指定されたパスを開く
-- @param spec table ファイラーの仕様
function M.open(spec)
  spec = spec or {}
  local roots = spec.roots
  if not roots or #roots == 0 then
    local log = require("UNL.logging").get(spec.logger_name or "UNL")
    log.warn("netrw provider: No roots specified to open.")
    return
  end

  -- 最初のルートのパスを対象とする
  local target_path = roots[1].path

  -- 1. パスにスペースや特殊文字が含まれていても安全にコマンドで使えるようにエスケープする
  local escaped_path = vim.fn.fnameescape(target_path)

  -- 2. :edit コマンドでnetrwを開く
  --    このコマンドはNeovimのCWDを変更せず、netrwのビューだけを指定パスで開く
  local cmd_string = "edit " .. escaped_path

  local log = require("UNL.logging").get(spec.logger_name or "UNL")
  log.info("Executing netrw command: %s", cmd_string)

  -- 3. コマンドを安全に実行
  local ok, err = pcall(vim.cmd, cmd_string)
  if not ok then
    log.error("Failed to open netrw: %s", tostring(err))
  end
end

return M
