-- lua/UNL/db.lua
local M = {}

local sqlite = require("sqlite")

--- データベースを開く（なければ作成・初期化）
--- @param opts table { namespace="UEP", name="MyProject", tables={...}, on_open=func }
function M.open(opts)
  opts = opts or {}
  local namespace = opts.namespace or "Common"
  local name = opts.name or "default"
  
  -- 1. 保存パスの生成
  local cache_base = vim.fn.stdpath("cache")
  local cache_dir = string.format("%s/%s", cache_base, namespace)
  local db_path = string.format("%s/%s.db", cache_dir, name)

  if vim.fn.isdirectory(cache_dir) == 0 then
    vim.fn.mkdir(cache_dir, "p")
  end

  -- 2. SQLite接続
  local db = sqlite.new(db_path)
  
  -- 3. オープン
  local ok, err = pcall(function() db:open() end)
  if not ok then
    error("UNL.db: Failed to open database at " .. db_path .. "\nError: " .. tostring(err))
    return nil
  end

  -- 4. テーブル作成 (存在しない場合のみ)
  if opts.tables then
    for tbl_name, schema in pairs(opts.tables) do
      -- 修正: 既にテーブルがあるかチェックする
      if not db:exists(tbl_name) then
        local create_ok, create_err = pcall(function()
          db:create(tbl_name, schema)
        end)
        
        if not create_ok then
          db:close()
          error("UNL.db: Failed to create table '" .. tbl_name .. "'.\nError: " .. tostring(create_err))
        end
      end
    end
  end

  -- 5. カスタム初期化処理
  if opts.on_open then
    local on_open_ok, on_open_err = pcall(opts.on_open, db)
    if not on_open_ok then
      vim.notify("UNL.db: Error in on_open callback: " .. tostring(on_open_err), vim.log.levels.WARN)
    end
  end

  return db
end

return M
