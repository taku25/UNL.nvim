local M = {}

---
-- 指定された名前のプロセスが現在実行中かを非同期でチェックします。
--
-- @param process_name string チェックしたいプロセスの実行ファイル名 (例: "UnrealEditor" or "UnrealEditor.exe")
-- @param on_complete function 結果を受け取るコールバック関数。引数は is_running (boolean) の1つ。
function M.is_process_running(opts)
  opts = opts or {}
  local cmd
  local args

  if vim.fn.has('win32') == 1 then
    -- Windowsの場合: `tasklist` を使用し、イメージ名でフィルタリングします。
    cmd = "cmd.exe"
    args = { "/c", "tasklist | findstr /i /b " .. opts.process_name }
  else
    -- Linux / macOSの場合: `pgrep` が最も効率的です。
    local search_name = opts.process_name
    cmd = "pgrep"
    args = { "-f", search_name }
  end

  local stdout_chunks = {}
  -- ▼▼▼【修正点】jobstartに渡す前にテーブルをフラット化する ▼▼▼
  local command_table = { cmd }
  vim.list_extend(command_table, args)
  vim.fn.jobstart(command_table, {
    stdout_buffered = true,
    on_stdout = function(_, data)
      if data then
        vim.list_extend(stdout_chunks, data)
      end
    end,
    on_exit = function(_, exit_code)
      local is_running = false
      if vim.fn.has('win32') == 1 then
        is_running = #stdout_chunks > 1 or (#stdout_chunks == 1 and stdout_chunks[1] ~= "")
      else
        is_running = (exit_code == 0)
      end

      if opts.on_complete then
        vim.schedule(function()
          opts.on_complete(is_running)
        end)
      end
    end,
  })
end

return M
