-- lua/UNL/backend/dynamic_picker/provider/fzf_lua.lua (jobstart version - Final)

local M = { name = "fzf-lua" }

-- モジュールのトップレベルで依存関係を読み込みます
local fzf_lua_ok, fzf_lua = pcall(require, "fzf-lua")
local builtin_ok, builtin = pcall(require, "fzf-lua.previewer.builtin")
local devicons_ok, devicons = pcall(require, "nvim-web-devicons")

-- プレビューアークラスをモジュールレベルで定義します
local DynamicPreviewer
if builtin_ok then
  DynamicPreviewer = builtin.buffer_or_file:extend()
  DynamicPreviewer._map = {}

  function DynamicPreviewer:new(o, opts, fzf_win)
    DynamicPreviewer.super.new(self, o, opts, fzf_win)
    setmetatable(self, DynamicPreviewer)
    return self
  end

  function DynamicPreviewer:parse_entry(entry_str)
    local file_path = DynamicPreviewer._map[entry_str]
    if file_path then return { path = file_path } end
    return {}
  end
end

function M.available()
  return fzf_lua_ok
end

function M.run(spec)
  spec = spec or {}
  assert(spec.command, "fzf-lua dynamic_picker requires 'spec.command'")

  local log = require("UNL.logging").get(spec.logger_name or "UNL")

  -- コマンドと引数をテーブルにまとめます
  local cmd_table = { spec.command }
  if spec.args and type(spec.args) == "table" then
    vim.list_extend(cmd_table, spec.args)
  end

  local lines = {}
  -- ▼▼▼【変更点】fn_transform の代わりに vim.fn.jobstart を使用します ▼▼▼
  vim.fn.jobstart(cmd_table, {
    cwd = spec.cwd,
    on_stdout = function(_, data)
      if data then
        for _, line in ipairs(data) do
          if line and line ~= "" then
            table.insert(lines, (line:gsub("\r", "")))
          end
        end
      end
    end,
    on_stderr = function(_, data)
      if data and data[1] and data[1] ~= "" then
        log.warn("fzf-lua dynamic_picker job stderr: %s", vim.inspect(data))
      end
    end,
    -- コマンドが正常に終了した後に、fzfを起動します
    on_exit = function(_, code)
      vim.schedule(function()
        if code ~= 0 then
          log.error("dynamic_picker script failed with exit code: %d", code)
          if spec.on_cancel then spec.on_cancel() end
          return
        end

        if #lines == 0 then
          log.warn("dynamic_picker script returned no items.")
          if spec.on_cancel then spec.on_cancel() end
          return
        end

        -- fn_transform内で行っていた処理をここに移動します
        if DynamicPreviewer then
          DynamicPreviewer._map = {}
        end

        local display_items = {}
        for _, line in ipairs(lines) do
          local parts = vim.split(line, "\t", { plain = true, trimempty = true })
          local display_text, file_path = parts[1], parts[2]

          if display_text and file_path then
            local icon_str = ""
            if devicons_ok and spec.devicons_enabled ~= false then
              local extension = vim.fn.fnamemodify(file_path, ":e")
              icon_str = (devicons.get_icon(file_path, extension) or "").. " "
            end

            local final_display_str = icon_str.. display_text
            table.insert(display_items, final_display_str)

            if DynamicPreviewer then
              DynamicPreviewer._map[final_display_str] = file_path
            end
          end
        end

        -- 加工済みのデータをfzfに渡します
        fzf_lua.fzf_exec(display_items, {
          prompt = spec.title or "Dynamic Items>",
          actions = {
            ["default"] = function(selected)
              if spec.on_submit and selected and #selected > 0 then
                local file_path = DynamicPreviewer and DynamicPreviewer._map[selected[1]]
                if file_path then
                  spec.on_submit(file_path)
                end
              end
            end,
            ["esc"] = function()
              if spec.on_cancel then spec.on_cancel() end
            end,
            ["ctrl-c"] = function()
              if spec.on_cancel then spec.on_cancel() end
            end,
          },
          previewer = (spec.preview_enabled ~= false and DynamicPreviewer) and DynamicPreviewer or nil,
        })
      end)
    end,
  })
end

return M
