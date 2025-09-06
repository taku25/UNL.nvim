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
    -- (将来、ここに :UNL clear_cache のようなコマンドが追加されるかもしれませんね)
  },
})
