local M = {}

local levels = vim.log.levels

function M.parse(level_str)
  if not level_str then return levels.INFO end
  local s = level_str:upper()
  return levels[s] or levels.INFO
end

function M.name(num)
  for k,v in pairs(levels) do
    if v == num then return k end
  end
  return tostring(num)
end

function M.visible(msg_level, threshold)
  threshold = threshold or levels.INFO
  if threshold == levels.OFF then return false end
  return msg_level >= threshold
end

function M.highlight(num)
  if num == levels.ERROR then return "ErrorMsg" end
  if num == levels.WARN  then return "WarningMsg" end
  if num == levels.DEBUG then return "SpecialComment" end
  if num == levels.TRACE then return "Comment" end
  return "Normal"
end

return M
