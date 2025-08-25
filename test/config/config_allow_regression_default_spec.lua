local helper = require("helper.config")

describe("UNL.config allow_regression default", function()
  local ctx

  before_each(function()
    ctx = helper.setup()
  end)

  after_each(function()
    helper.teardown(ctx)
  end)

  it("is false by default", function()
    local cfg = helper.cfg()
    assert.is_false(cfg.ui.progress.allow_regression)
  end)

  it("can be enabled via user config", function()
    helper.teardown(ctx)
    ctx = helper.setup({
      user = { ui = { progress = { allow_regression = true } } },
    })
    local cfg = helper.cfg()
    assert.is_true(cfg.ui.progress.allow_regression)
  end)
end)
