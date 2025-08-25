-- 最初に setup を必ず読む (package.path と vim stub を確立)
require("helper.setup_progress")

local util = require("helper.progress_test_util")
local progress = util.get_progress_module()
local registry = progress.registry

describe("capabilities filtering", function()
  before_each(function()
    util.reset_and_reload()
    registry.reset_auto_chain_refresh()
  end)

  it("require_capabilities filters providers (window)", function()
    -- まず provider 一覧を確認
    local list = registry.providers_list()
    -- print(vim.inspect(list))  -- デバッグ用
    local win_spec = registry.get("window")
    assert.is_truthy(win_spec, "window_refresh spec missing (provider not loaded)")
    local ok, detected = pcall(win_spec.detect or function() return true end)
    if not (ok and detected) then
      pending("window_refresh detect failed; skipping")
      return
    end

    local spec, name = registry.resolve{
      category = "progress",
      ui = {
        progress = {
          mode = "auto",
        },
      },
      require_capabilities = { "window" },
      context = { purpose = "refresh" },
    }
    
    assert.equals("window", name)
    assert.is_truthy(spec)
  end)

  it("capabilities mismatch falls back to dummy", function()
    local spec, name = registry.resolve{
      category = "progress",
      ui = {
        progress = {
          mode = "auto",
        },
      },
      require_capabilities = { "no_such_cap" },
      context = { purpose = "refresh" },
    }
    assert.equals("dummy", name)
    assert.is_truthy(spec)
  end)
end)
