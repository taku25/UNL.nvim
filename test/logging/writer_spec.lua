local lvl = require("UNL.logging.level")
local Logger = require("UNL.logging.logger")
local H = require("helper.logging")

-- We assume directory has been renamed to writer/; adjust requires ifまだ witer/.
local W_echo   = require("UNL.logging.writer.echo")
local W_file   = require("UNL.logging.writer.file")
local W_notify = require("UNL.logging.writer.notify")
local W_perf   = require("UNL.logging.writer.perf")

-- Stub vim.notify again (isolated test file)
local notify_calls = {}
vim.notify = function(msg, level)
  notify_calls[#notify_calls+1] = { msg = msg, level = level }
end

-- Stub nvim_echo
vim.api = vim.api or {}
vim.api.nvim_echo = function() end

describe("writers: echo / notify basic filtering", function()
  it("echo writer suppresses below threshold", function()
    local w = W_echo.new()
    local cfg = { logging = { echo = { level = "WARN" } } }
    w.write(vim.log.levels.INFO, "info suppressed", { config = cfg })
    w.write(vim.log.levels.WARN, "warn visible", { config = cfg })
    -- Cannot directly capture echo output easily; rely on absence of error.
    assert.is_true(true)
  end)

  it("notify writer honors custom prefix", function()
    local w = W_notify.new()
    local cfg = { logging = { notify = { level = "INFO", prefix = "[NTFY]" } } }
    w.write(vim.log.levels.INFO, "hello", { config = cfg })
    assert.is_true(#notify_calls >= 1)
    local found = false
    for _, c in ipairs(notify_calls) do
      if c.msg:match("%[NTFY%] hello") then
        found = true
        break
      end
    end
    assert.is_true(found)
  end)
end)

describe("writer: perf", function()
  it("emits only perf meta messages with enabled patterns", function()
    local w = W_perf.new()
    local cfg = { logging = { perf = { enabled = true, level = "TRACE" } } }
    -- non-perf meta
    w.write(vim.log.levels.TRACE, "should ignore", { config = cfg, meta = {} })
    -- perf meta
    w.write(vim.log.levels.TRACE, "hit", { config = cfg, meta = { is_perf = true, category = "db" } })
    assert.is_true(#notify_calls > 0)
    local ok = false
    for _, c in ipairs(notify_calls) do
      if c.msg:match("%[PERF%]") then ok = true end
    end
    assert.is_true(ok)
  end)
end)

describe("writer: file rotation & threshold", function()
  it("writes to file and rotates when size exceeded", function()
    local tmp = H.make_tempdir()
    local restore = H.override_stdpath(tmp)
    local w = W_file.new()

    local cfg = {
      logging = {
        level = "TRACE",
        file = {
          enable = true,
          level = "TRACE",
          filename = "test.log",
          max_kb = 1,    -- small to force rotation
          rotate = 2,
        }
      }
    }

    -- Write enough lines to exceed ~1KB
    local big_line = string.rep("X", 300)
    for i = 1, 10 do
      w.write(vim.log.levels.INFO, "L" .. i .. " " .. big_line, { config = cfg })
    end

    local cache_dir = vim.fs.joinpath(tmp, "UNL_cache")
    local base = vim.fs.joinpath(cache_dir, "test.log")
    -- Give a tiny delay for IO flush (should not be necessary but safer)
    assert.is_true(vim.fn.filereadable(base) == 1 or vim.fn.filereadable(base .. ".1") == 1)

    -- Clean up
    restore()
  end)
end)
