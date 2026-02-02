local unl_finder = require("UNL.finder")
local unl_remote_kismet = require("UNL.remote.kismet")
local cmd_setup = require("UNL.cmd.setup")
local cmd_start = require("UNL.cmd.start")
local cmd_refresh = require("UNL.cmd.refresh")
local cmd_watch = require("UNL.cmd.watch")
local cmd_cleanup = require("UNL.cmd.cleanup")
local cmd_status = require("UNL.cmd.status")
local cmd_cd = require("UNL.cmd.cd")
local cmd_delete = require("UNL.cmd.delete")

local M = {}

function M.get_server_status_info()
  local server = require("UNL.scanner.server")
  if server.is_running() then
    return "UNL: OK"
  else
    return "UNL: OFF"
  end
end

function M.get_progress_component()
  local ok, progress_status = pcall(require, "UNL.backend.progress.status")
  if not ok then return "" end
  local status = progress_status.get()
  if not status or not status.active then return "" end
  local spinner_chars = { "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏" }
  local spinner_index = (math.floor(vim.loop.hrtime() / 1e8)) % #spinner_chars + 1
  local spinner = spinner_chars[spinner_index]
  return string.format("%s %s %3d%%%% %s", status.title, spinner, status.percentage, status.message)
end

function M.find_project(file_path, opts) return unl_finder.project.find_project(file_path, opts) end
function M.find_module(file_path, opts) return unl_finder.module.find_module(file_path, opts) end
function M.find_engine(project_file_path, opts) return unl_finder.engine.find_engine_root(project_file_path, opts) end
function M.find_insights(file_path, opts) return unl_finder.insights.find(file_path, opts) end
function M.kismet_command(opts) return unl_remote_kismet.execute(opts) end

function M.is_process_running(opts)
  local unl_process_util = require("UNL.process.util")
  unl_process_util.is_process_running(opts)
end

function M.toggle_debug_log()
  local debug_log = require("UNL.backend.buf.debug_log")
  debug_log.toggle()
end

M.provider = require("UNL.provider")
M.scanner = require("UNL.scanner")
M.db = require("UNL.db")
M.project = require("UNL.project")

function M.setup(opts) cmd_setup.execute(opts) end
function M.start(opts) cmd_start.execute(opts) end
function M.refresh(opts) cmd_refresh.execute(opts) end
function M.watch(opts) cmd_watch.execute(opts) end
function M.cleanup(opts) cmd_cleanup.execute(opts) end
function M.server_status(opts) cmd_status.execute(opts) end
function M.cd(opts) cmd_cd.execute(opts) end
function M.delete(opts) cmd_delete.execute(opts) end

function M.register_client()
  require("UNL.scanner.server").register_self()
end

return M