-- UNL/lua/UNL/event/types.lua
-- UNLエコシステム全体で共有される、グローバルなイベントの型定義です。
-- プラグイン間の連携は、ここで定義されたイベントを通じて行われます。
--
-- 命名規則:
--   - プレフィックスとして "unl:" を付け、グローバルイベントであることを明示します。
--   - キー名は UPPER_SNAKE_CASE で統一します。

local M = {
  ----------------------------------------------------------------------
  -- UEP.nvim (データプロバイダー) が発行するイベント
  ----------------------------------------------------------------------

  ---
  -- UProjectの論理ツリー構造のデータが更新されたときに発行されます。
  -- @param nodes table: neo-treeが解釈できるノードのリストがペイロードとして渡されます。
  ON_UPROJECT_TREE_UPDATE = "unl:on_uproject_tree_update",

  ---
  -- :UEP refresh が完了したときに発行されます。
  -- @param result table: { status = "success" | "failed" } がペイロードとして渡されます。
  ON_AFTER_REFRESH_COMPLETED = "unl:on_after_refresh_completed",

  ---
  -- プロジェクトキャッシュ (.uproject) の保存が完了したときに発行されます。
  ON_AFTER_PROJECT_CACHE_SAVE = "unl:on_after_project_cache_save",

  ---
  -- ファイルキャッシュ (files.json) の保存が完了したときに発行されます。
  ON_AFTER_FILE_CACHE_SAVE = "unl:on_after_file_cache_save",

  ----------------------------------------------------------------------
  -- UCM.nvim (コードジェネレーターなど) が発行する可能性のあるイベント
  ----------------------------------------------------------------------

  ---
  -- 新しいソースファイルが作成されたときに発行されます。
  -- @param file_path string: 作成されたファイルのフルパスがペイロードとして渡されます。
  ON_SOURCE_FILE_CREATED = "unl:on_source_file_created",

  ---
  -- ソースファイルが削除されたときに発行されます。
  -- @param file_path string: 削除されたファイルのフルパスがペイロードとして渡されます。
  ON_SOURCE_FILE_DELETED = "unl:on_source_file_deleted",

 ---
  -- プラグインのセットアップが完了したことを通知するイベント。
  -- @param plugin_info table: { name = "plugin_name" } がペイロードとして渡される。
  ON_PLUGIN_AFTER_SETUP = "unl:on_plugin_after_setup",}

-- このテーブルを凍結して、意図しない変更を防ぐ
return M
