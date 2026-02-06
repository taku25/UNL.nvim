-- lua/UNL/backend/dynamic_stack_picker/provider/snacks.lua

local M = { name = "snacks" }

function M.available()
  return _G.Snacks and _G.Snacks.picker
end

function M.run(spec)
  spec = spec or {}
  local snacks = _G.Snacks.picker
  
  local results_queue = {}
  local is_done = false

  -- 最初の一件（ダミー）を入れて、UIが閉じないようにする
  table.insert(results_queue, { text = "Searching...", virtual = true })

  -- push関数の定義
  local push = function(items)
    if not items then return end
    
    local to_add = {}
    local add_fn = function(item)
      local entry = { text = "" }
      if type(item) == "table" then
        entry.text = tostring(item.display or item.label or item.filename or item.value or "")
        entry.value = item.value or item
        entry.file = item.filename
      else
        entry.text = tostring(item)
        entry.value = item
      end
      
      if entry.text == "" then entry.text = " " end
      table.insert(to_add, entry)
    end

    if type(items) == "table" and items[1] ~= nil then
      for _, it in ipairs(items) do add_fn(it) end
    else
      add_fn(items)
    end

    -- キューに追加
    for _, it in ipairs(to_add) do
      table.insert(results_queue, it)
    end
  end

  -- ピッカー起動前にデータ生成を開始
  if spec.start then
    spec.start(push)
  end

  -- Snacks Picker を起動
  snacks.pick({
    title = spec.title or "Dynamic Stack",
    -- フォーマッタを明示的に指定（デフォルトが file だと文字列が消えることがあるため）
    format = "text",
    finder = function()
      ---@async
      return function(cb)
        local Async = require("snacks.picker.util.async")
        
        -- ループし続けてデータを流し込む
        while not is_done do
          -- キューから取り出して一件ずつ cb に渡す
          -- ★重要: cb(batch) ではなく cb(item) を呼ぶ必要がある
          while #results_queue > 0 do
            local item = table.remove(results_queue, 1)
            cb(item)
          end
          
          -- 10ms スリープ（UIスレッドに制御を戻す）
          Async.sleep(10)
        end
      end
    end,
    confirm = function(p, item)
      p:close()
      is_done = true
      if item and spec.on_submit then
        spec.on_submit(item.value or item.text)
      end
    end
  })
end

return M