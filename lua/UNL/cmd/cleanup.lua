-- lua/UNL/cmd/cleanup.lua (Core Data Management)
local finder = require("UNL.finder")
local path_util = require("UNL.path")
local log = require("UNL.logging").get("UNL")

local M = {}

function M.execute(opts)
  opts = opts or {}
  
  -- プロジェクトルートの特定
  local project_info = finder.project.find_project(vim.loop.cwd())
  if not (project_info and project_info.root) then
    return log.error("Cleanup: Could not find project root. Are you in an Unreal project?")
  end
  local project_root = project_info.root
  local project_display_name = vim.fn.fnamemodify(project_root, ":t")

  -- ユーザーへの確認
  local prompt_str = string.format("Permanently delete ALL UNL DB records for project '%s'?", project_display_name)
  if vim.fn.confirm(prompt_str, "&Yes\n&No", 2) ~= 1 then
    return
  end

  local db_path = path_util.get_db_path(project_root)
  if vim.fn.filereadable(db_path) == 1 then
    local removed, err = os.remove(db_path)
    if removed then
      log.info("Deleted DB file: %s", db_path)
      -- SQLite関連ファイルの削除
      os.remove(db_path .. "-wal")
      os.remove(db_path .. "-shm")
      log.info("Cleanup complete for %s", project_display_name)
    else
      log.error("Failed to delete DB file: %s", tostring(err))
    end
  else
    log.warn("DB file not found: %s", db_path)
  end
end

return M