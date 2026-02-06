-- lua/UNL/backend/dynamic_stack_picker/provider/fzf_lua.lua

local M = { name = "fzf_lua" }

function M.available()
  return pcall(require, "fzf-lua")
end

function M.run(spec)
  spec = spec or {}
  local fzf = require("fzf-lua")
  local log = require("UNL.logging").get(spec.logger_name or "UNL")

  local fzf_fn = function(fzf_cb)
    -- push関数の定義
    local push = function(items)
      if not items then return end
      
      local add_item = function(item)
        local line
        if type(item) == "table" then
          line = item.display or item.label or item.filename or tostring(item.value)
        else
          line = tostring(item)
        end
        -- fzf_cb に文字列を渡す (あるいはリッチなエントリ)
        fzf_cb(line)
      end

      if type(items) == "table" and items[1] ~= nil then
        for _, it in ipairs(items) do add_item(it) end
      else
        add_item(items)
      end
    end

    -- 処理開始 (fzf-luaは別スレッド/プロセスのような扱いでこの関数を呼ぶ)
    if spec.start then
      spec.start(push)
    end
    
    -- pushが終わったことを知らせるために、start側で完了させる必要があるが、
    -- fzf-luaの場合は nil を送ると終了。
    -- ただし非同期RPCの場合はいつ終わるか分からないので、start内で明示的に
    -- 終わらせる仕組みが必要かもしれないが、一旦ここでは放置。
  end

  local opts = {
    prompt = (spec.title or "Stack") .. "> ",
    actions = {
      ["default"] = function(selected)
        if selected and #selected > 0 then
          local val = selected[1]
          if spec.on_submit then
            spec.on_submit(val)
          end
        end
      end
    }
  }

  fzf.fzf_exec(fzf_fn, opts)
end

return M
