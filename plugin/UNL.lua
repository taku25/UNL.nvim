-- plugin/UNL.lua

-- このファイルは、Neovim起動時に自動的に読み込まれ、コマンドを登録します。

-- 安全のため、pcallでラップするのが定石です
local ok, builder = pcall(require, "UNL.command.builder")
if not ok then
  vim.notify("UNL.nvim: Failed to load command builder. Commands will not be available.", vim.log.levels.ERROR)
  return
end

local api_ok, api = pcall(require, "UNL.api")
if not api_ok then
  vim.notify("UNL.nvim: Failed to load API. Commands will not be available.", vim.log.levels.ERROR)
  return
end

-- UNL.command.builder を使って、コマンドを定義
builder.create({
  plugin_name = "UNL",
  cmd_name = "UNL",
  desc = "UNL: Core library commands",
  subcommands = {
    ["debuglog"] = {
      handler = function(opts)
        -- api.toggle_debug_log() を呼び出す
        api.toggle_debug_log()
      end,
      desc = "Toggle the unified debug log viewer for all UNL plugins.",
      args = {}, -- このコマンドは引数を取らない
    },
    ["setup"] = {
      handler = api.setup,
      desc = "Setup UNL project database.",
      args = {},
    },
    ["start"] = {
      handler = api.start,
      desc = "Start UNL services (Boot).",
      args = {},
    },
    ["refresh"] = {
      handler = api.refresh,
      bang = true,
      desc = "Refresh UNL project database.",
      args = {{ name = "scope", required = false }},
    },
    ["watch"] = {
      handler = api.watch,
      desc = "Start UNL file watcher explicitly.",
      args = {},
    },
    ["cleanup"] = {
      handler = api.cleanup,
      desc = "Cleanup UNL project database.",
      args = {},
    },
    ["status"] = {
      handler = api.server_status,
      desc = "Show UNL server and project status.",
      args = {},
    },
    ["cd"] = {
      handler = api.cd,
      desc = "Select a registered project and cd to it.",
      args = {},
    },
    ["delete"] = {
      handler = api.delete,
      desc = "Delete a project from the UNL registry.",
      args = {},
    },
    -- (将来、ここに :UNL clear_cache のようなコマンドが追加されるかもしれませんね)
  },
})

-- Auto-start Server Logic
vim.api.nvim_create_autocmd({ "VimEnter", "DirChanged" }, {
  group = vim.api.nvim_create_augroup("UNL_AutoStart", { clear = true }),
  callback = function()
    local ok_conf, conf = pcall(function() return require("UNL.config").get("UNL") end)
    if ok_conf and conf.remote and conf.remote.auto_server_start then
      local unl_finder = require("UNL.finder")
      local project_info = unl_finder.project.find_project(vim.loop.cwd())
      if project_info and project_info.uproject then
        require("UNL.scanner.server").ensure_running(function(running)
           -- Optional: Notify or log if started automatically
        end)
      end
    end
  end
})
