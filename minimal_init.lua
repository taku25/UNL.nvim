-- minimal_init.lua (finalized)
local root = vim.fn.fnamemodify(vim.fn.expand("<sfile>"), ":p:h")
vim.opt.runtimepath:append(root)

-- LuaRocks (busted) path merge
if vim.env.LUA_PATH and not package.path:find(vim.env.LUA_PATH, 1, true) then
  package.path = package.path .. ";" .. vim.env.LUA_PATH
end
if vim.env.LUA_CPATH and not package.cpath:find(vim.env.LUA_CPATH, 1, true) then
  package.cpath = package.cpath .. ";" .. vim.env.LUA_CPATH
end

-- Add plugin & test module search paths
package.path = table.concat({
  root .. "/lua/?.lua",
  root .. "/lua/?/init.lua",
  root .. "/test/?.lua",
  root .. "/test/?/init.lua",
  package.path,
}, ";")

-- Backward compat: old logger path

-- Helper alias (if specs still use require('test.helper.config'))
-- package.preload["test.helper.config"] = function()
--   return require("helper.config")
-- end
--
-- package.preload["test.helper.finder"] = function()
--   return require("helper.finder")
-- end
--
-- package.preload["test.helper.logging"] = function()
--   return require("helper.logging")
-- end

-- Optional eager logger init (safe to ignore failures)
pcall(function()
  require("UNL.logging.init").setup({
    logging = {
      level = "info",
      echo = { level = "warn" },
      file = { enable = false },
      notify = { level = "error", prefix = "[UNL]" },
      perf = { enabled = false },
    },
  })
end)
