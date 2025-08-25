-- Ensure vim stub exists BEFORE requiring plugin modules
require("helper.vim_stub")

-- Inject repo lua/ into package.path (Windows対応)
local sep = package.config:sub(1,1)  -- "\" or "/"
local function norm(p)
  if sep == "\\" then
    return (p:gsub("/", "\\"))
  else
    return (p:gsub("\\", "/"))
  end
end

-- 推定: このファイルは <repo>/test/helper/setup_progress.lua
local this = debug.getinfo(1, "S").source  -- "@C:\path\to\repo\test\helper\setup_progress.lua"
local repo_root = this:match("^@(.+)[/\\]test[/\\]helper[/\\]setup_progress.lua$")
if not repo_root then
  -- フォールバック: test ディレクトリまでで止める
  repo_root = this:match("^@(.+)[/\\]test[/\\].+$") or "."
end
repo_root = norm(repo_root)

local lua_root = repo_root .. sep .. "lua"
local patterns = {
  lua_root .. sep .. "?.lua",
  lua_root .. sep .. "?\\init.lua", -- Windows
  lua_root .. "/?/init.lua",        -- Unix fallback
  lua_root .. "/?.lua",
}
local add_path = table.concat(patterns, ";")
if not package.path:find(lua_root, 1, true) then
  package.path = add_path .. ";" .. package.path
end

-- Option: デバッグ表示
-- print("[setup_progress] package.path extended with: " .. add_path)

return {
  repo_root = repo_root,
  lua_root = lua_root,
}
