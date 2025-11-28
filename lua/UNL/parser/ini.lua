-- lua/UNL/parser/ini.lua
local M = {}

--- INIファイルを解析する
--- @param filepath string ファイルパス
--- @return table|nil { sections = { [SectionName] = { { key=..., value=..., op=..., line=... }, ... } } }
function M.parse(filepath)
  if vim.fn.filereadable(filepath) == 0 then return nil end
  
  local lines = vim.fn.readfile(filepath)
  local data = { sections = {} }
  local current_section = nil
  
  for i, line in ipairs(lines) do
    -- コメント除去 (;以降)
    local content = line:match("^([^;]*)")
    content = vim.trim(content)
    
    if content ~= "" then
      -- [Section] の検出
      local section_name = content:match("^%[([^%]]+)%]$")
      if section_name then
        current_section = section_name
        if not data.sections[current_section] then
          data.sections[current_section] = {}
        end
      elseif current_section then
        -- Key=Value の検出 (演算子 +, -, . ! も考慮)
        -- 例: r.SetRes=1920x1080, +ActionMappings=..., !ActionMappings=ClearArray
        
        local op, key, value = content:match("^([%+%-%.!]?)([^=]+)=(.*)$")
        
        if key and value then
          key = vim.trim(key)
          -- 値の前後の引用符はここでは外さず、生のまま保持する方針（必要なら後で加工）
          
          table.insert(data.sections[current_section], {
            key = key,
            value = value,
            op = op, -- "+" or "-" or "." or "!" or "" (empty for set/override)
            line = i,
            raw = line
          })
        end
      end
    end
  end
  
  return data
end

return M
