
local Aggregator = require("UNL.backend.progress.aggregator")
-- 状態を管理するモジュールを読み込む
local status_manager = require("UNL.backend.progress.status")

local spec = {
  -- プロバイダー名
  name = "generic_status",
  
  -- プログレスバックエンドのカテゴリ
  category = "progress",
  
  -- 優先順位（fidgetやwindowよりは低く、dummyよりは高い）
  weight = 80, 
  
  -- このプロバイダーが持つ能力
  capabilities = { 
    statusline = true -- ステータスラインでの表示に適していることを示す
  },

  -- 常に利用可能
  detect = function() 
    return true 
  end,
  
  -- プログレスバーのインスタンスを作成する関数
 create = function(opts)
    if opts.enabled == false then return nil end
    local aggr = Aggregator.new(opts.weights)
    
    local r = {}
    function r:open()
      status_manager.set({ active = true, percentage = 0, message = "Starting...", title = opts.title or "Task" })
    end
    function r:stage_define(name, total)
      aggr:define(name, total)
      status_manager.set({ message = "define: " .. name, percentage = aggr:percentage() })
    end
    function r:stage_update(name, done, msg)
      aggr:update(name, done)
      status_manager.set({ message = msg or ("update: " .. name), percentage = aggr:percentage() })
    end
    function r:update(stage, message)
      status_manager.set({ message = message or stage, percentage = aggr:percentage() })
    end
    function r:finish(success)
      if success then
        status_manager.set({ message = "Done!", percentage = 100 })
      else
        status_manager.set({ message = "Failed!", percentage = aggr:percentage() })
      end
      vim.defer_fn(function()
        status_manager.set({ active = false })
      end, 2000)
    end
    return r
  end,
}

return spec
