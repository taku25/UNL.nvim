local Path = require("UNL.path")

local M = {}

---
-- UHTのManifest.jsonファイルのパスを探索または構築する
-- @param opts table 探索オプション
--   - project_root (string) プロジェクトのルートディレクトリ
--   - project_name (string) プロジェクト名 (拡張子なし)
--   - target_preset (table) UBTのターゲットプリセット { Platform, Configuration, IsEditor }
-- @return string|nil 見つかったManifest.jsonのフルパス、またはnil
function M.find_uht_manifest(opts)
  if not (opts and opts.project_root and opts.project_name and opts.target_preset) then
    return nil, "find_uht_manifest: invalid opts"
  end

  -- ターゲット名（例: MyProjectEditor）をプリセットから構築
  local target_name = opts.target_preset.IsEditor and (opts.project_name .. "Editor") or opts.project_name

  -- Unreal Engineの標準的なパス構造に基づいて、Manifest.jsonのパスを直接組み立てる
  local manifest_path = Path.join(
    opts.project_root,
    "Intermediate",
    "Build",
    opts.target_preset.Platform,
    target_name,
    opts.target_preset.Configuration,
    target_name..".uhtmanifest"
  )

  -- 組み立てたパスにファイルが実際に存在するかを確認
  if vim.fn.filereadable(manifest_path) == 1 then
    return manifest_path, nil
  end

  return nil, "UHT manifest not found at expected location."
end

return M
