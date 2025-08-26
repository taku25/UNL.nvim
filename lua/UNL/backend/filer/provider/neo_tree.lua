local M = { name = "neo-tree" }

function M.available()
  return pcall(require, "neo-tree.command")
end

---
-- neo-treeを開く
-- @param spec table ファイラーの仕様
--   - roots: table (単一または複数のルート情報)
-- ただし neo-treeの オプションである  bind_to_cwd = false が必要
function M.open(spec)
  spec = spec or {}
  local roots = spec.roots
  if not roots or #roots == 0 then
    -- (ログ出力など)
    return
  end

  local target_path = roots[1].path

  local ok, neotree_cmd = pcall(require, "neo-tree.command")
  if not ok then return end

  neotree_cmd.execute({
    source = "filesystem", -- 標準のファイルシステムソースを使う
    dir  = target_path,
  })
end

return M
