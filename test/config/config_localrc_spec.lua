local helper = require("helper.config")

describe("UNL.config local rc merging", function()
  local ctx

  before_each(function()
    ctx = helper.setup({
      user = {
        logging = { level = "debug" },
      },
      localrc = {
        logging = { level = "error", file = { enable = false } },
        ui = { progress = { mode = "notify" } },
      },
    })
  end)

  after_each(function()
    helper.teardown(ctx)
  end)

  it("local rc overrides user config", function()
    local cfg = helper.cfg()
    assert.are.equal("error", cfg.logging.level)
    assert.is_false(cfg.logging.file.enable)
    assert.are.equal("notify", cfg.ui.progress.mode)
  end)
end)
