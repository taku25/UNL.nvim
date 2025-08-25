local helper = require("helper.config")

describe("UNL.config user override", function()
  local ctx

  before_each(function()
    ctx = helper.setup({
      user = {
        logging = { level = "debug" },
        ui = { progress = { mode = "window" } },
      },
    })
  end)

  after_each(function()
    helper.teardown(ctx)
  end)

  it("applies user overrides", function()
    local cfg = helper.cfg()
    assert.are.equal("debug", cfg.logging.level)
    assert.are.equal("window", cfg.ui.progress.mode)
  end)
end)
