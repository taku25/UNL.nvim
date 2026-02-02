local Aggregator = require("UNL.backend.progress.aggregator")

local spec = {
  name = "notify",
  category = "progress",
  weight = 70,
  capabilities = {
    notify     = true,
    percentage = true,
  },
  detect = function()
    return type(vim) == "table" and type(vim.notify) == "function"
  end,
  create = function(opts)
    if opts.enabled == false then return nil end
    local aggr = Aggregator.new(opts.weights)
    local throttle_ms = opts.throttle_ms or 50 -- Reduced from 120
    local title = opts.title or "UNL Refresh"
    local last = 0

    local function emit(stage)
      local pct = aggr:percentage()
      local msg = string.format("%s %d%% (%s)", title, pct, stage or "")
      vim.notify(msg, vim.log.levels.INFO, { title = title })
    end

    local r = {}
    function r:stage_define(name, total)
      aggr:define(name, total)
      emit("define:" .. name)
    end
    function r:stage_update(name, done, msg)
      aggr:update(name, done)
      local now = vim.loop.hrtime() / 1e6
      if now - last >= throttle_ms then
        last = now
        emit(msg or ("update:" .. name))
      end
    end
    function r:update(stage, message)
      emit(message or stage)
    end
    function r:finish(success)
      local final = success and (title .. " completed (100%)") or (title .. " failed")
      vim.notify(final, success and vim.log.levels.INFO or vim.log.levels.ERROR, { title = title })
    end
    return r
  end,
}

return spec
