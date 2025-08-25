local lvl = require("UNL.logging.level")

describe("UNL.logging.level", function()
  it("parses known names (case-insensitive)", function()
    assert.equals(vim.log.levels.INFO, lvl.parse("info"))
    assert.equals(vim.log.levels.ERROR, lvl.parse("ERROR"))
    assert.equals(vim.log.levels.DEBUG, lvl.parse("DeBuG"))
  end)

  it("defaults to INFO for unknown", function()
    assert.equals(vim.log.levels.INFO, lvl.parse("???"))
    assert.equals(vim.log.levels.INFO, lvl.parse(nil))
  end)

  it("name() round-trips known levels", function()
    for k,v in pairs(vim.log.levels) do
      assert.equals(k, lvl.name(v))
    end
  end)

  it("visible() enforces threshold", function()
    local T = vim.log.levels
    assert.is_true(lvl.visible(T.ERROR, T.INFO))
    assert.is_false(lvl.visible(T.TRACE, T.WARN))
  end)

  it("highlight() returns expected highlight groups", function()
    local T = vim.log.levels
    assert.is_string(lvl.highlight(T.ERROR))
    assert.is_string(lvl.highlight(T.WARN))
  end)
end)
