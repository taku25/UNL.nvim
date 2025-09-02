local M = {}

-- { [event_name] = { callback1, callback2, ... } }
local subscribers = {}

---
-- イベントを購読（登録）する
-- @param event_name string 購読したいイベントの名前
-- @param callback function イベント発生時に呼び出される関数
function M.subscribe(event_name, callback)
  if not subscribers[event_name] then
    subscribers[event_name] = {}
  end
  -- 念のため、同じコールバックが複数登録されないようにしても良い
  table.insert(subscribers[event_name], callback)
end


-- イベントを発行（通知）する
-- コールバックは、次のUIティックで安全に実行される
-- @param event_name string
-- @param ... any
function M.publish(event_name, ...)
  if subscribers[event_name] and #subscribers[event_name] > 0 then
    -- 引数をキャプチャするために、ローカル変数に一旦保存する
    local args = { ... }
    
    vim.schedule(function()
      -- vim.scheduleの時点でもう一度チェックするのが安全
      if not subscribers[event_name] then return end

      for _, callback_function in ipairs(subscribers[event_name]) do
        -- ★★★ 修正箇所 ★★★
        -- callback_function は関数そのもの
        -- pcall(callback_function, unpack(args))
        callback_function(unpack(args))
      end
    end)
  end
end

return M
