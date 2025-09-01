local M = {}

local current_status = {
  active = false,
  percentage = 0,
  message = "",
  title = "Task",
}

function M.set(new_status)
  current_status = vim.tbl_deep_extend("force", current_status, new_status)
end

---
-- 外部から現在のプログレス状態を直接取得するための公開API
-- @return table { active, percentage, message, title }
function M.get()
  return current_status
end

-- get_progress_info のような整形関数はここには置かない

return M
