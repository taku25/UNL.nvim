-- lua/UNL/parser/init.lua
local M = {}

-- 既存のCPPパーサー
local cpp_parser
do
  local ok, mod = pcall(require, "UNL.parser.cpp")
  if ok then cpp_parser = mod end
end

-- ★ 新規: INIパーサー
local ini_parser
do
  local ok, mod = pcall(require, "UNL.parser.ini")
  if ok then ini_parser = mod end
end

M.cpp = cpp_parser
M.ini = ini_parser -- ★ 追加

return M
