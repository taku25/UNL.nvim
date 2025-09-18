-- lua/UNL/backend/dynamic_picker/provider/fzf_lua.lua (Streaming version)

local M = { name = "fzf-lua" }

local fzf_lua_ok, fzf_lua = pcall(require, "fzf-lua")
local builtin_ok, builtin = pcall(require, "fzf-lua.previewer.builtin")
local devicons_ok, devicons = pcall(require, "nvim-web-devicons")

-- プレビューアークラスをモジュールレベルで定義します
local DynamicPreviewer
if builtin_ok then
  DynamicPreviewer = builtin.buffer_or_file:extend()
  -- 対応表をクラス自体に持たせることで、関数のスコープを超えて永続化させます
  DynamicPreviewer._map = {}

  function DynamicPreviewer:new(o, opts, fzf_win)
    DynamicPreviewer.super.new(self, o, opts, fzf_win)
    setmetatable(self, DynamicPreviewer)
    return self
  end

  function DynamicPreviewer:parse_entry(entry_str)
    -- クラスにアタッチされた永続的なマップを参照します
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

  local cmd_table = { spec.command }
  if spec.args and type(spec.args) == "table" then
    vim.list_extend(cmd_table, spec.args)
  end

  -- ▼▼▼【変更点】fzf_exec に関数を渡すストリーミング方式に変更 ▼▼▼
  local provider_fn = function(write_to_pipe)
    vim.fn.jobstart(cmd_table, {
      cwd = spec.cwd,
      on_stdout = function(_, data)
        if data then
          for _, line in ipairs(data) do
            if line and line ~= "" then
              -- 1行ずつ受け取り、その場で加工します
              local clean_line = line:gsub("\r", "")
              local parts = vim.split(clean_line, "\t", { plain = true, trimempty = true })
              local display_text, file_path = parts[1], parts[2]

              if display_text and file_path then
                local icon_str = ""
                if devicons_ok and spec.devicons_enabled ~= false then
                  local extension = vim.fn.fnamemodify(file_path, ":e")
                  icon_str = (devicons.get_icon(file_path, extension) or "").. " "
                end
                local final_display_str = icon_str.. display_text

                -- プレビューとアクションのためにマップを更新します
                if DynamicPreviewer then
                  DynamicPreviewer._map[final_display_str] = file_path
                end

                -- 加工した行をfzfのプロセスに書き込みます
                write_to_pipe(final_display_str)
              end
            end
          end
        end
      end,
      on_stderr = function(_, data)
        if data and data[1] and data[1] ~= "" then
          log.warn("fzf-lua dynamic_picker job stderr: %s", vim.inspect(data))
        end
      end,
      on_exit = function(_, code)
        -- ストリームの終了をfzfに伝えます
        write_to_pipe(nil)

        if code ~= 0 then
          vim.schedule(function()
            log.error("dynamic_picker script failed with exit code: %d", code)
            if spec.on_cancel then spec.on_cancel() end
          end)
        end
      end,
    })
  end

  -- 実行のたびに永続マップをクリアします
  if DynamicPreviewer then
    DynamicPreviewer._map = {}
  end

  -- fzf_execを関数プロバイダーで実行します
  fzf_lua.fzf_exec(provider_fn, {
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
end

return M
