-- lua/UNL/parser/init.lua
local M = {}

-- CPPパーサーはRustサーバー(unl-server)に統合されました。
-- 現在ここにはINIパーサーのみが残っています。

local ini_parser
do
  local ok, mod = pcall(require, "UNL.parser.ini")
  if ok then ini_parser = mod end
end

M.ini = ini_parser

return M