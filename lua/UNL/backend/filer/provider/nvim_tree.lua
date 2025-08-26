local M = { name = "nvim-tree" }

function M.available()
  return pcall(require, "nvim-tree")
end

-- nvim-treeを指定ディレクトリで開く
-- @param spec table
--   - roots: table (単一または複数のルート情報)
--      update_cwd = false,          -- cwd自動連動なし
--      respect_buf_cwd = false,     -- バッファのcwdは無視
--      sync_root_with_cwd = false,  -- nvim本体cwd変化にも追従しない
function M.open(spec)
  spec = spec or {}
  local roots = spec.roots
  if not roots or #roots == 0 then
    return
  end

  local target_path = roots[1].path

  local ok, api = pcall(require, "nvim-tree.api")
  if not ok then return end

  -- nvim-treeウィンドウを開く（まだ開いてなければ）
  api.tree.open()

  -- ルートを移動する
  api.tree.change_root(target_path)
end

return M
