local project_finder = require("UNL.finder.project")
local engine_resolver = require("UNL.finder.engine")
local path = require("UNL.path")
local log = require("UNL.logging").get("UNL")

local M = {}

---
-- 現在のOSに合わせたプラットフォーム固有のディレクトリ名を返す
-- @return string "Win64", "Mac", "Linux" など
local function get_platform_binary_dir()
  -- vim.loop.os_uname() はOSの情報をくれる便利な機能
  local sysname = vim.loop.os_uname().sysname
  if sysname == "Windows_NT" then
    return "Win64"
  elseif sysname == "Darwin" then -- DarwinはmacOSのカーネル名
    return "Mac"
  else
    -- 将来的にLinuxにも対応できるよう、フォールバックを用意
    return "Linux"
  end
end

---
-- 現在のOSに合わせた実行ファイル名を返す
-- @return string "UnrealInsights.exe" または "UnrealInsights"
local function get_insights_executable_name()
  if vim.loop.os_uname().sysname == "Windows_NT" then
    return "UnrealInsights.exe"
  else
    return "UnrealInsights"
  end
end

function M.find(start_path, opts)
  opts = opts or {}
  if start_path == nil or start_path == "" then start_path = vim.loop.cwd() end

  local project_info = project_finder.find_project(start_path, opts)
  if not project_info then
    return nil, ("finder.insights: Could not find a .uproject file from '%s'."):format(start_path)
  end
  log.trace("finder.insights: Found project at '%s'", project_info.uproject)

  local engine_root, err = engine_resolver.find_engine_root(project_info.uproject, opts)
  if not engine_root then
    return nil, ("finder.insights: Could not resolve engine root. Reason: %s"):format(tostring(err))
  end
  log.trace("finder.insights: Resolved engine root at '%s'", engine_root)
  
  
  -- 1. プラットフォーム固有のディレクトリ名と実行ファイル名を取得
  local platform_dir = get_platform_binary_dir()
  local executable_name = get_insights_executable_name()

  -- 2. それらを使って、OSに応じた正しいパスを組み立てる
  local insights_path = path.join(engine_root, "Engine", "Binaries", platform_dir, executable_name)


  if vim.fn.executable(insights_path) ~= 1 then
    return nil, ("finder.insights: UnrealInsights not found or not executable at '%s'"):format(insights_path)
  end

  log.info("Found UnrealInsights at: %s", insights_path)
  return insights_path, nil
end

return M
