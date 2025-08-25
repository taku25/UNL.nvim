local helper = require("helper.config")

describe("UNL.config project.localrc_filename override", function()
  local ctx

  before_each(function()
    ctx = helper.setup({
      user = { project = { localrc_filename = ".custom_unlrc.json" } },
      -- localrc must match the overridden filename
      -- helper.setup writes using what Config.get().project.localrc_filename returns (after user setup)
      localrc = { logging = { level = "error" } },
    })
  end)

  after_each(function()
    helper.teardown(ctx)
  end)

  it("respects overridden localrc filename", function()
    local cfg = helper.cfg()
    assert.are.equal("error", cfg.logging.level)
    assert.are.equal(".custom_unlrc.json", cfg.project.localrc_filename)
  end)
end)
