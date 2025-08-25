local Logger = require("UNL.logging.logger")
local lvlmod = require("UNL.logging.level")
local H = require("helper.logging")

-- Stub out vim.notify / vim.api.nvim_echo to avoid UI noise
local notify_calls = {}
vim.notify = function(msg, level)
  notify_calls[#notify_calls+1] = { msg = msg, level = level }
end

vim.api = vim.api or {}
vim.api.nvim_echo = function(chunks, history, opts)
  -- capture but ignore for tests
end

describe("UNL.logging.logger basic", function()

  before_each(function()
    Logger:_reset()
    notify_calls = {}
  end)

  it("emits all basic levels via writers", function()
    local writer, store = H.collecting_writer()
    local config = {
      logging = {
        level = "TRACE",
        file = { enable = false },
        echo = { level = "TRACE" },
        notify = { level = "TRACE" },
      }
    }
    local log = Logger:create("UNL",{
      prefix = "[T]",
      writers = { writer },
      config_getter = function() return config end,
    })
    log.trace("hello %s", "trace")
    log.debug("hello debug")
    log.info("hello info")
    log.warn("hello warn")
    log.error("hello error")

    assert.equals(5, #store)
    assert.equals("[T] hello trace", store[1].msg)
    assert.equals("TRACE", store[1].ctx.meta.level_name)
    assert.equals("ERROR", store[#store].ctx.meta.level_name)
  end)

  it("formats with string.format only if args provided", function()
    local writer, store = H.collecting_writer()
    local config = { logging ={
      level = "INFO",
        echo = { enabled = true, level = "INFO" },
      }
    }
    local log = Logger:create("",{
      prefix = "",
      writers = { writer },
      config_getter = function() return config end,
    })
    log.info("No format tokens here %s") -- accidental %s (no arg) -> kept literal
    log.info("Value=%d", 7)
    assert.equals(2, #store)
    assert.matches("No format tokens here %%s", store[1].msg)
    assert.matches("Value=7", store[2].msg)
  end)

  it("perf logging respects enabled=false", function()
    local writer, store = H.collecting_writer()
    local cfg = {
      logging = {
        perf = { enabled = false, level = "TRACE", patterns = { "db" } },
      }
    }
    local log = Logger:create("UNL",{
      prefix = "[P]",
      writers = { writer },
      config_getter = function() return cfg end,
    })
    log.perf("db.query", "cost=%dms", 12)
    assert.equals(0, #store)
  end)

  it("perf logging matches patterns and caches them", function()
    local writer, store = H.collecting_writer()
    local dynamic_cfg = {
      logging = {
        perf = { enabled = true, level = "DEBUG", patterns = { "^db%." } },
      }
    }
    local log = Logger:create("UNL",{
      prefix = "[PERF]",
      writers = { writer },
      config_getter = function() return dynamic_cfg end,
    })
    -- Should match
    log.perf("db.query", "t=%d", 10)
    -- Should not match
    log.perf("net.http", "t=%d", 5)
    assert.equals(1, #store)
    assert.matches("%[PERF%] t=10", store[1].msg)

    -- Update patterns (add net)
    dynamic_cfg.logging.perf.patterns = { "^db%.", "^net%." }
    log.perf("net.http", "ok")
    assert.equals(2, #store)
  end)

  it("perf logging ignores invalid patterns (does not throw)", function()
    local writer, store = H.collecting_writer()
    local cfg = {
      logging = {
        perf = { enabled = true, level = "TRACE", patterns = { "(" } }, -- invalid
      }
    }
    local log = Logger:create("UNL",{
      prefix = "[PERF]",
      writers = { writer },
      config_getter = function() return cfg end,
    })
    log.perf("anything", "should not match")
    assert.equals(0, #store)
  end)

end)
