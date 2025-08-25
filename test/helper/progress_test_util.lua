-- 既存内容の冒頭に追加
require("helper.vim_stub")

local progress = require("UNL.backend.progress")
local registry = progress.registry

local M = {}

function M.reset_and_reload()
  progress._reset_providers()
  progress._load_providers()
  registry.reset_auto_chain_refresh()
end

function M.dump_providers()
  return registry.providers_list()
end

function M.add_temp_provider(name, spec_override)
  local spec = {
    name = name,
    category = "progress",
    weight = spec_override.weight or 40,
    capabilities = spec_override.capabilities or { temp = true },
    detect = spec_override.detect or function() return true end,
    create = function(opts)
      local calls = {}
      local obj = {
        _calls = calls,
        stage_define = function(self, st, total)
          calls[#calls+1] = { "stage_define", st, total }
        end,
        stage_update = function(self, st, done, msg)
          calls[#calls+1] = { "stage_update", st, done, msg }
        end,
        update = function(self, stage, msg)
          calls[#calls+1] = { "update", stage, msg }
        end,
        finish = function(self, ok)
          calls[#calls+1] = { "finish", ok }
        end,
      }
      return obj
    end,
  }
  for k, v in pairs(spec_override or {}) do
    spec[k] = v
  end
  registry.register(spec)
end

function M.get_progress_module()
  return progress
end

function M.pop_notify_calls()
  return _G.__TEST_VIM_NOTIFY_POP and _G.__TEST_VIM_NOTIFY_POP() or {}
end

return M
