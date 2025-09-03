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

  ON_AFTER_CHANGE_DIRECTORY = "unl:on_after_change_directory",

  ON_AFTER_DELETE_PROJECT_REGISTRY = "nul:on_after_delete_project_registry",
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
  ON_PLUGIN_AFTER_SETUP = "unl:on_plugin_after_setup",



  -- UnrealのbuildStepが完了したときに呼ばれる
  -- @param plugin_info table: { name = "plugin_name" } がペイロードとして渡される。
  ON_AFTER_BUILD = "unl:on_after_build",

  -- Unrealのgenerateclangdatabaseが完了したときに呼ばれる
  ON_AFTER_GENERATE_COMPILE_DATABASE = "unl:on_generate_compile_database",

  -- Unrealのgenerateheaderが完了したときに呼ばれる
  ON_AFTER_GENERATE_HEADER = "unl:on_generate_header",

  -- Unrealのgenerate projectが完了したときに呼ばれる
  ON_AFTER_GENERATE_PROEJCT = "unl:on_after_generate_proejct",

  -- Unrealの静的解析が完了したときに呼ばれる
  ON_AFTER_LINT = "unl:on_after_lint",


  -- Delete で実際にファイルが削除されたときに呼ばれる
  ON_AFTER_DELETE_CLASS_FILE = "unl:on_after_delete_class_file",

  --新しいクラスファイルが削除されたときに呼ばれる
  ON_AFTER_NEW_CLASS_FILE = "unl:on_after_new_class_file",

  --クラスファイルが移動されたときに呼ばれる
  ON_AFTER_MOVE_CLASS_FILE = "unl:on_after_move_class_file",

  --クラスがリネイムされたときに呼ばれる
  ON_AFTER_RENAME_CLASS_FILE = "unl:on_after_rename_class_file",

 --- ★ この新しいイベントを定義
  ON_REQUEST_UPROJECT_TREE_VIEW = "nil:on_request_uproject_tree_view",


  ON_AFTER_LOG_VIEWER_START = "nil:on_after_log_viewer_start",
  ON_AFTER_LOG_VIEWER_STOP = "nil:on_after_log_viewer_stop",
}

-- このテーブルを凍結して、意図しない変更を防ぐ
return M
