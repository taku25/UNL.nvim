-- lua/UNL/finder/insights.lua

local project_finder = require("UNL.finder.project")
local engine_resolver = require("UNL.finder.engine")
local path = require("UNL.path")
local log = require("UNL.logging").get("UNL")

local M = {}

---
-- UnrealInsights.exe のパスを同期的に検索する
-- @param start_path string 検索を開始するパス
-- @param opts? table | nil 検索オプション (finder.project や finder.engine に渡される)
-- @return string|nil, string|nil (insights_path, err_msg)
function M.find(start_path, opts)
  opts = opts or {}
  if start_path == nil or start_path == "" then start_path = vim.loop.cwd() end

  -- 1. まず、現在の場所からプロジェクト(.uproject)を見つける
  -- --- ★ 修正点1: find_project はエラー文字列を返さないため、戻り値の有無でチェックします ---
  local project_info = project_finder.find_project(start_path, opts)
  if not project_info then
    return nil, ("finder.insights: Could not find a .uproject file from '%s'."):format(start_path)
  end
  -- --- ★ 修正点2: loggerのタイポ修正 (otps -> opts, rtace -> trace) と、モジュール統一のlogを使用 ---
  log.trace("finder.insights: Found project at '%s'", project_info.uproject)

  -- 2. 見つけた.uprojectファイルを使って、関連するEngineのルートを解決する
  -- --- ★ 修正点3: 未定義の 'find_opts' ではなく、引数で受け取った 'opts' を渡します ---
  local engine_root, err = engine_resolver.find_engine_root(project_info.uproject, opts)
  if not engine_root then
    return nil, ("finder.insights: Could not resolve engine root. Reason: %s"):format(tostring(err))
  end
  log.trace("finder.insights: Resolved engine root at '%s'", engine_root)
  
  -- 3. EngineルートからUnrealInsights.exeのフルパスを組み立てる
  local insights_path = path.join(engine_root, "Binaries", "Win64", "UnrealInsights.exe")

  -- 4. 実行ファイルとして存在するかを最終検証
  if vim.fn.executable(insights_path) ~= 1 then
    return nil, ("finder.insights: UnrealInsights.exe not found or not executable at '%s'"):format(insights_path)
  end

  log.info("Found UnrealInsights.exe at: %s", insights_path)

  -- --- ★ 修正点4: 成功した場合、エラーメッセージとして nil を返すのがLuaの慣習です ---
  return insights_path, nil
end

return M
