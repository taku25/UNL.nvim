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
    local function throttled(msg)
      local now = vim.loop.hrtime() / 1e6
      if now - last >= throttle_ms then
        last = now
        local total_pct = aggr:percentage()
        local stage_info = aggr:current_stage_info()
        
        local display_pct = total_pct
        local display_msg = msg
        
        if stage_info then
          -- ステージ内の進捗率を表示のメインにする
          display_pct = math.floor(stage_info.ratio * 100 + 0.5)
          -- メッセージに全体の進捗を添える
          display_msg = string.format("[%d%%] %s", total_pct, msg or stage_info.name)
        end
        
        handle:report({ percentage = display_pct, message = display_msg })
      end
    end

    local r = {}
    function r:stage_define(name, total)
      aggr:define(name, total)
      throttled("define:" .. name)
    end
    function r:stage_update(name, done, msg)
      aggr:update(name, done)
      throttled(msg or ("update:" .. name))
    end
    function r:update(stage, message)
      throttled(message or stage)
    end
    function r:finish(success)
      if success then handle:finish() else handle:cancel() end
    end
    return r
  end,
}

return spec
