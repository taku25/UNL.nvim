-- lua/UNL/cache/core.lua

local fs = require("vim.fs")
local json = vim.json
local log -- 遅延読み込み

local has_dkjson, dkjson = pcall(require, "dkjson")
local M = {}

-- プラグインのキャッシュ用ベースディレクトリを取得する
function M.get_cache_dir(conf)
  local base = vim.fn.stdpath("cache")
  local dirname = (conf.cache and conf.cache.dirname) or "UNL_CACHE"
  return fs.joinpath(base, dirname)
end

-- 汎用的なJSON保存関数
function M.save_json(path, data)
  
  local dir = fs.dirname(path)
  if vim.fn.isdirectory(dir) == 0 then
    pcall(vim.fn.mkdir, dir, "p")
  end
  
  -- ★★★ ステップ2: dkjson が利用可能かチェック ★★★
  local ok, encoded
  if has_dkjson then
    -- dkjson があれば、インデント付きで整形する
    ok, encoded = pcall(dkjson.encode, data, { indent = true })
  else
    -- なければ、標準の vim.json を使う
    ok, encoded = pcall(json.encode, data, { indent = "  " })
  end
  
  if not ok then
    return false, "Failed to encode JSON for path %s: %s", path, tostring(encoded)
  end
  
  local file, err = io.open(path, "w")
  if not file then
    return false, "Failed to open file for writing %s: %s", path, tostring(err)
  end
  
  file:write(encoded)
  file:close()
  return true
end

-- ★★★ 今回追加する、汎用的なJSON読み込み関数 ★★★
function M.load_json(path)
  -- 1. ファイルが存在し、読み込み可能かチェック
  if vim.fn.filereadable(path) == 0 then
    return nil, "File not readable"
  end
  
  -- 2. ファイルの内容を読み込む
  local file, err = io.open(path, "r")
  if not file then
    return nil, "Failed to open file for reading %s: %s", path, tostring(err)
  end
  local content = file:read("*a")
  file:close()
  
  -- 3. JSON文字列をLuaテーブルに変換 (pcallで安全に実行)
  local ok, data = pcall(json.decode, content)
  if not ok then
    return nil, "Failed to decode JSON from path %s: %s", path, tostring(data)
  end
  
  return data
end

return M
