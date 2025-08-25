-- lua/UNL/analyzer/build_cs.lua (改良版)

local fn = vim.fn
local M = {}

---
-- 文字列の中から引用符で囲まれたモジュール名を全て抽出するヘルパー
-- "Core", "Engine", "InputCore" -> { "Core", "Engine", "InputCore" }
-- @param blob string
-- @return string[]
--
local function extract_quoted_strings(blob)
  local result = {}
  if not blob then return result end
  for token in blob:gmatch('"(.-)"') do
    table.insert(result, token)
  end
  return result
end

---
-- .Build.cs ファイルから依存関係を解析する
-- @param filepath string
-- @return table { public: string[], private: string[] }
--
function M.parse(filepath)
  local result = { public = {}, private = {} }
  if fn.filereadable(filepath) ~= 1 then
    return result
  end

  local ok, lines = pcall(fn.readfile, filepath)
  if not ok then return result end
  local content = table.concat(lines, "\n")
  
  -- コメントを簡易的に除去（より堅牢にするにはさらなる改良が必要）
  content = content:gsub("//.-\n", "\n")
  content = content:gsub("/%*.-%*/", "")

  -- 1. PublicDependencyModuleNames の解析
  -- 1a. AddRange(...) 形式を全て見つける
  for blob in content:gmatch('PublicDependencyModuleNames%.AddRange%s*%([^)]+{(.-)}[^)]*%)') do
    vim.list_extend(result.public, extract_quoted_strings(blob))
  end
  -- 1b. Add(...) 形式を全て見つける
  for blob in content:gmatch('PublicDependencyModuleNames%.Add%((.-)%)') do
    vim.list_extend(result.public, extract_quoted_strings(blob))
  end

  -- 2. PrivateDependencyModuleNames の解析
  -- 2a. AddRange(...) 形式を全て見つける
  for blob in content:gmatch('PrivateDependencyModuleNames%.AddRange%s*%([^)]+{(.-)}[^)]*%)') do
    vim.list_extend(result.private, extract_quoted_strings(blob))
  end
  -- 2b. Add(...) 形式を全て見つける
  for blob in content:gmatch('PrivateDependencyModuleNames%.Add%((.-)%)') do
    vim.list_extend(result.private, extract_quoted_strings(blob))
  end

  return result
end

return M
