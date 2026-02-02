-- lua/UNL/utils.lua (Core Utilities)
local M = {}

--- パスの正規化とスラッシュ統一
function M.normalize_path(path)
  if not path then return nil end
  return path:gsub("\\", "/")
end

--- カテゴリ判定 (Unreal Engine 向け)
function M.categorize_path(path)
  if path:match("%.uproject$") then return "uproject" end
  if path:match("%.uplugin$") then return "uplugin" end
  
  if path:find("/Programs/", 1, true) or path:match("/Programs$") then return "programs" end
  if path:find("/Shaders/", 1, true) or path:match("/Shaders$") then return "shader" end
  if path:find("/Config/", 1, true) or path:match("/Config$") then return "config" end
  if path:find("/Content/", 1, true) or path:match("/Content$") then return "content" end
  if path:find("/Source/", 1, true) or path:match("/Source$") then return "source" end
  
  return "other"
end

--- 相対パスの作成
function M.create_relative_path(file_path, base_path)
  if not file_path or not base_path then return file_path end
  local norm_file = file_path:gsub("\\", "/")
  local norm_base = base_path:gsub("\\", "/")
  local file_parts = vim.split(norm_file, "/", { plain = true })
  local base_parts = vim.split(norm_base, "/", { plain = true })
  local common_len = 0
  for i = 1, math.min(#file_parts, #base_parts) do
    if file_parts[i]:lower() == base_parts[i]:lower() then common_len = i else break end
  end
  if common_len > 0 and common_len < #file_parts then
    local relative_parts = {}
    for i = common_len + 1, #file_parts do table.insert(relative_parts, file_parts[i]) end
    return table.concat(relative_parts, "/")
  end
  return file_path
end

--- 指定パスが含まれるモジュールを特定
function M.find_module_for_path(file_path, all_modules_map)
  if not file_path or not all_modules_map then return nil end
  local normalized_path = file_path:gsub("\\", "/")
  local best_match = nil
  local longest_path = 0
  for _, module_meta in pairs(all_modules_map) do
    if module_meta.module_root then
      local normalized_root = module_meta.module_root:gsub("\\", "/")
      if normalized_path:find(normalized_root, 1, true) and #normalized_root > longest_path then
        longest_path = #normalized_root
        best_match = module_meta
      end
    end
  end
  return best_match
end

return M
