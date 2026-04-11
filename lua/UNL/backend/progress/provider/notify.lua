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
    local throttle_ms = opts.throttle_ms or 50
    local title = opts.title or "UNL Refresh"
    local last = 0

    local function emit(name, done, total)
      local msg = aggr:format(name, done, total)
      vim.notify(string.format("%s  %s", title, msg), vim.log.levels.INFO, { title = title })
    end

    local r = {}
    function r:define_from_plan(phases)
      aggr:define_from_plan(phases)
    end
    function r:stage_define(name, total)
      aggr:define(name, total)
      emit(name, 0, total)
    end
    function r:stage_update(name, done, total, msg)
      aggr:update(name, done, total)
      local now = vim.loop.hrtime() / 1e6
      if now - last >= throttle_ms then
        last = now
        emit(name, done, total)
      end
    end
    function r:update(stage, message)
      emit(stage, nil, nil)
    end
    function r:finish(success)
      local final = success and (title .. "  Complete (100%)") or (title .. "  Failed")
      vim.notify(final, success and vim.log.levels.INFO or vim.log.levels.ERROR, { title = title })
    end
    return r
  end,
}

return spec
