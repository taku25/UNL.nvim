-- lua/UNL/backend/find_picker/provider/fzf_lua.lua

local M = { name = "fzf-lua" }

function M.available()
  return pcall(require, "fzf-lua")
end

function M.run(spec)
  spec = spec or {}
  local log = require("UNL.logging").get(spec.logger_name or "UNL")

  if not (spec.exec_cmd and type(spec.exec_cmd) == "table" and #spec.exec_cmd > 0) then
    log.error("fzf-lua find_picker: exec_cmd table is required.")
    return
  end

  local fzf_lua = require("fzf-lua")
  local fzf_config = require("fzf-lua.config")
  -- (追加) カスタムプレビューワーのために builtin を読み込む
  local builtin = require("fzf-lua.previewer.builtin")

  local cmd_parts = {}
  for _, part in ipairs(spec.exec_cmd) do
    table.insert(cmd_parts, vim.fn.shellescape(part))
  end
  local cmd_string = table.concat(cmd_parts, " ")
  
  log.debug("fzf-lua find_picker: Wrapping command string: %s", cmd_string)

  local fzf_opts = {
    prompt = spec.title or "Find Results>",
    actions = {
      ["default"] = function(selected)
        if spec.on_submit and selected and #selected > 0 then
          pcall(spec.on_submit, selected[1])
        end
      end,
      ["ctrl-c"] = function()
        if spec.on_cancel then pcall(spec.on_cancel) end
      end,
    },
  }

  -- ▼▼▼ ここからがプレビュー設定の変更箇所 ▼▼▼
  if spec.preview_enabled then
    -- 1. `picker` と同じように、カスタムプレビューワーを定義する
    local FindPickerPreviewer = builtin.buffer_or_file:extend()

    function FindPickerPreviewer:new(o, opts, fzf_win)
      FindPickerPreviewer.super.new(self, o, opts, fzf_win)
      setmetatable(self, FindPickerPreviewer)
      return self
    end
    
    -- 2. fzf-luaから渡されたエントリー文字列を解釈する関数
    function FindPickerPreviewer:parse_entry(entry_str)
      -- `fd` が返すのはファイルパスそのものなので、
      -- 渡された文字列をそのままファイルパスとして返すだけで良い
      if entry_str and type(entry_str) == 'string' then
        return {
          path = entry_str,
        }
      end
      -- もし解釈できなければ空のテーブルを返す
      return {}
    end
    
    -- 3. 作成したカスタムプレビューワーを `previewer` オプションに設定
    fzf_opts.previewer = FindPickerPreviewer
    -- (オプション) レイアウトを明示的に指定すると、より確実
    -- fzf_opts.preview_win = 'right:50%'
  end
  -- ▲▲▲ 変更ここまで ▲▲▲

  local normalized_opts = fzf_config.normalize_opts(fzf_opts, {})
  fzf_lua.fzf_wrap(cmd_string, normalized_opts)
end

return M
