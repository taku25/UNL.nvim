local unl_db = require("UNL.db")

-- スキーマ定義 (sqlite.lua の仕様準拠)
local schema = {
  files = {
    id = { "integer", "primary", "key" },
    path = { "text", "required", "unique" },
    mtime = { "integer", "required" },
  },
  classes = {
    id = { "integer", "primary", "key" },
    file_id = { "integer", "reference", "files.id", "on_delete", "cascade" },
    name = { "text", "required" },
    parent = { "text" },
  }
}

-- DBを開く
local db = unl_db.open({
  namespace = "UEP",
  name = "MyProject_Cache", -- プロジェクトごとに名前を変える想定
  tables = schema,
})

if db then
  -- データの挿入
  db:insert("files", { path = "/path/to/Actor.h", mtime = 123456 })
  
  -- データの検索
  local results = db:select("files", { path = "/path/to/Actor.h" })
  
  -- 終了時
  db:close()
end
