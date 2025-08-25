local helper = require("helper.config")

describe("UNL.config defaults", function()
  local ctx

  before_each(function()
    ctx = helper.setup()
  end)

  after_each(function()
    helper.teardown(ctx)
  end)

  it("loads default progress settings", function()
    local cfg = helper.cfg()
    assert.is_truthy(cfg.ui)
    assert.is_truthy(cfg.ui.progress)
    assert.are.equal("auto", cfg.ui.progress.mode)
    assert.is_true(cfg.ui.progress.enable)
    assert.is_false(cfg.ui.progress.allow_regression)
  end)

  it("has logging defaults", function()
    local cfg = helper.cfg()
    assert.are.equal("info", cfg.logging.level)
    assert.is_true(cfg.logging.file.enable)
    assert.are.same({"^refresh"}, cfg.logging.perf.patterns)
  end)

  it("cache defaults", function()
    local cfg = helper.cfg()
    assert.are.equal("UNL_cache", cfg.cache.dirname)
  end)
end)
