local util = require("helper.progress_test_util")
local progress = util.get_progress_module()
local registry = progress.registry

describe("registry basic resolution", function()
  before_each(function()
    util.reset_and_reload()
    registry.reset_auto_chain_refresh()
  end)

  it("auto chain chooses a non-dummy provider when available", function()
    local spec, name = registry.resolve{
      category = "progress",
      ui = "auto",
      context = { purpose = "refresh" },
    }
    assert.is_truthy(spec)
    assert.is_truthy(name)
    assert.not_equal("dummy", name, "expected some provider other than dummy (notify or window) if environment supports it")
  end)

  it("set_auto_chain_refresh affects order", function()
    -- Force dummy to be first
    registry.set_auto_chain_refresh({ "dummy", "notify_refresh" })
    local spec, name = registry.resolve{
      category = "progress",
      progress = {
      mode = "auto",
    },
      context = { purpose = "refresh" },
    }
    assert.equals("dummy", name)
    -- Reset and ensure dummy is no longer forced first
    registry.reset_auto_chain_refresh()
    local spec2, name2 = registry.resolve{
      category = "progress",
      progress = {
      mode = "auto",
    },
      context = { purpose = "refresh" },
    }
    assert.not_equal(nil, name2)
  end)

  it("disable forces dummy", function()
    local spec, name = registry.resolve{
      category = "progress",
      progress = {
      mode = "auto",
    },
      disable = true,
      context = { purpose = "refresh" },
    }
    assert.equals("dummy", name)
  end)

  it("explicit single ui picks that provider", function()
    -- notify_refresh should exist in environment (vim.notify stubbed)
    local spec, name = registry.resolve{
      category = "progress",
      mode = "notify",
      context = { purpose = "refresh" },
    }
    assert.equals("notify", name)
  end)
end)
