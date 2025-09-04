-- lua/UNL/backend/grep_picker/provider/dummy.lua
-- 何もせず、ログに警告を出すだけの最終フォールバック用プロバイダー
local M = { name = "dummy" }

-- ダミーは常に利用可能
function M.available()
  return true
end

---
-- ダミーピッカーの実行関数
-- UIは表示せず、ログに警告を出力する
-- @param opts table | nil 呼び出し元から渡されるオプション
function M.pick(opts)
  -- UNLのロガーを安全に取得する
  local log = require("UNL.logging").get("UNL")

  -- ユーザーには直接通知せず、ログに警告を残す
  log.warn("Grep Picker dummy provider was used. No UI was shown. Please install 'telescope.nvim' or 'fzf-lua'.")

  -- (オプション) もし呼び出し元が on_cancel コールバックを提供していれば、それを呼び出す
  if opts and opts.on_cancel and type(opts.on_cancel) == "function" then
    pcall(opts.on_cancel)
  end
end

-- レジストリへの登録は行わない。init.lua がこのファイルを require する責務を持つ。
return M
