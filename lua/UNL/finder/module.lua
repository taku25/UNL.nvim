-- lua/UNL/finder/module.lua
-- 「モジュール」探索の専門家 (単一モジュール)。

local ancestor = require("UNL.finder.ancestor")

local M = {}

--- .build.cs ファイルを持つモジュールルートを探します。
-- @param start_path string
-- @param opts table|nil
-- @return string|nil
function M.find_module_root(start_path, opts)
  -- ancestor.find_up_forward が正しい名称
  return ancestor.find_up(start_path, { "%.[Bb]uild%.cs$" }, opts)
end

return M
