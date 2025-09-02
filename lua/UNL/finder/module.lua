local ancestor = require("UNL.finder.ancestor")
local fs = require("vim.fs")

local M = {}

---
-- .Build.cs ファイルを見つけるためのチェッカー関数

---
-- .Build.cs ファイルを1つだけ見つけて、そのフルパスを返す
function M.find_build_cs(module_root)
  if not module_root then return nil end
  local ok, iter = pcall(fs.dir, module_root)
  if not ok then return nil end
  for name, type in iter do
    if type == "file" and name:match("%.[Bb]uild%.cs$") then
      return fs.joinpath(module_root, name)
    end
  end
  return nil
end

---
-- .build.cs ファイルを持つモジュールルートを探します。
function M.find_module_root(start_path, opts)
  local function build_cs_checker(dir)
    local ok, iter = pcall(fs.dir, dir)
    if not ok then return nil end
    for name, type in iter do
      if type == "file" and name:match("%.[Bb]uild%.cs$") then
        return dir
      end
    end
    return nil
  end
  return ancestor.find_with_checker(start_path, build_cs_checker, opts)
end


---
-- 指定されたパスから、関連するモジュールの全ての情報（ルート、ファイルパス、名前）を取得する
-- @param start_path string
-- @param opts table|nil
-- @return table|nil { root, module, name } の形のテーブル、またはnil
function M.find_module(start_path, opts)
  -- 1. モジュールルートを探す
  local module_root = M.find_module_root(start_path, opts)
  if not module_root then return nil end

  -- 2. .Build.cs のフルパスを探す
  local build_cs_path = M.find_build_cs(module_root)
  if not build_cs_path then return nil end

  -- 3. ファイル名から純粋なモジュール名を抽出する
  local build_cs_filename = vim.fn.fnamemodify(build_cs_path, ":t")
  local module_name = build_cs_filename:gsub("%.[Bb]uild%.cs$", "")

  -- 4. 全ての情報をテーブルにまとめて返す
  return {
    root = module_root,
    module = build_cs_path,
    name = module_name,
  }
end

return M
