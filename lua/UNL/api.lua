local unl_finder = require("UNL.finder")
local unl_remote_kismet = require("UNL.remote.kismet")
local M = {}


function M.get_progress_component()
  local ok, progress_status = pcall(require, "UNL.backend.progress.status")
  if not ok then return "" end
  
  local status = progress_status.get()
  
  if not status or not status.active then return "" end
  
  local spinner_chars = { "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏" }
  local spinner_index = (math.floor(vim.loop.hrtime() / 1e8)) % #spinner_chars + 1
  local spinner = spinner_chars[spinner_index]

  -- パーセント記号は statusline のために %% とエスケープする必要がある
  return string.format("%s %s %3d%%%% %s", status.title, spinner, status.percentage, status.message)
end



function M.find_project(file_path, opts)
  return unl_finder.project.find_project(file_path, opts)
end

function M.find_module(file_path, opts)
  return unl_finder.module.find_module(file_path, opts)
end

function M.find_engine(project_file_path, opts)
  return unl_finder.engine.find_engine_root(project_file_path, opts)
end

function M.find_insights(file_path, opts)
  return unl_finder.insights.find(file_path, opts)
end

function M.kismet_command(opts)
  return unl_remote_kismet.execute(opts)
end


-- UNLの統合デバッグログビューワーの表示/非表示を切り替える
function M.is_process_running(opts)
  local unl_process_util = require("UNL.process.util")
  unl_process_util.is_process_running(opts)
end


-- @usage `require("UNL.api").provider.register(...)`
-- @usage `local ok, result = require("UNL.api").provider.request(...)`
-- @usage `require("UNL.api").provider.notify(...)`
M.provider = require("UNL.provider")

return M
