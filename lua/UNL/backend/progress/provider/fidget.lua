local Aggregator = require("UNL.backend.progress.aggregator")

local spec = {
  name = "fidget",
  category = "progress",
  weight = 30,
  capabilities = {
    percentage = true,
    lsp_style  = true,
    rich_ui    = true,
  },
  detect = function()
    local ok = pcall(require, "fidget.progress")
    return ok
  end,
  create = function(opts)
    if opts.enabled == false then return nil end
    local ok, fidget_progress = pcall(require, "fidget.progress")
    if not ok then return nil end

    local aggr = Aggregator.new(opts.weights)
    local handle = fidget_progress.handle.create({
      title = opts.title or "Task",
      lsp_client = { name = opts.client_name },
    })

    local throttle_ms = opts.throttle_ms or 80
    local last = 0
    local function throttled(name, done, total)
      local now = vim.loop.hrtime() / 1e6
      if now - last >= throttle_ms then
        last = now
        -- fidget が percentage を表示するので、message 側にはパーセントを含めない
        local display_msg = aggr:format_no_pct(name, done, total)
        handle:report({ percentage = aggr:percentage(), message = display_msg })
      end
    end

    local r = {}
    function r:define_from_plan(phases)
      aggr:define_from_plan(phases)
    end
    function r:stage_define(name, total)
      aggr:define(name, total)
      throttled(name, 0, total)
    end
    function r:stage_update(name, done, total, msg)
      aggr:update(name, done, total)
      throttled(name, done, total)
    end
    function r:update(stage, message)
      throttled(stage, nil, nil)
    end
    function r:finish(success)
      if success then handle:finish() else handle:cancel() end
    end
    return r
  end,
}

return spec
