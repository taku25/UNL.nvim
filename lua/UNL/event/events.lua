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

---
-- イベントを発行（通知）する
-- @param event_name string 発行するイベントの名前
-- @param ... any コールバック関数に渡す可変長引数
function M.publish(event_name, ...)
  if subscribers[event_name] then
    for _, callback in ipairs(subscribers[event_name]) do
      -- pcall でラップして、一つのコールバックのエラーが他に影響しないようにする
      pcall(callback, ...)
    end
  end
end

return M
