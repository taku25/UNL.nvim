local helper = require("helper.config")

describe("UNL.config reload", function()
  local ctx

  before_each(function()
    ctx = helper.setup({
      localrc = {
        ui = { progress = { mode = "window" } },
      },
    })
  end)

  after_each(function()
    helper.teardown(ctx)
  end)

  it("changes when local rc file is modified and reload called", function()
    local Config = require("UNL.config")
    local cfg = helper.cfg()
    assert.are.equal("window", cfg.ui.progress.mode)

    -- Modify local rc
    assert.is_not_nil(ctx.rc_path)
    local new_rc = {
      ui = { progress = { mode = "notify" } },
    }
    local enc = (vim.json and vim.json.encode or vim.fn.json_encode)(new_rc)
    vim.fn.writefile(vim.split(enc, "\n"), ctx.rc_path)

    Config.reload("UNL", ctx.root .. "/dummy.cpp")
    local cfg2 = helper.cfg()
    assert.are.equal("notify", cfg2.ui.progress.mode)
  end)
end)
