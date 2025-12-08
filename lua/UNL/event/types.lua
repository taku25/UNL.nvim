-- UNL/lua/UNL/event/types.lua

local M = {
  ----------------------------------------------------------------------
  -- UEP.nvim (データプロバイダー) が発行するイベント
  ----------------------------------------------------------------------
  ON_UPROJECT_TREE_UPDATE = "unl:on_uproject_tree_update",
  ON_AFTER_REFRESH_COMPLETED = "unl:on_after_refresh_completed",
  ON_AFTER_PROJECT_CACHE_SAVE = "unl:on_after_project_cache_save",
  ON_AFTER_FILE_CACHE_SAVE = "unl:on_after_file_cache_save",
  ON_AFTER_UEP_LIGHTWEIGHT_REFRESH = "unl:on_after_uep_lightweight_refresh",
  ON_AFTER_CHANGE_DIRECTORY = "unl:on_after_change_directory",
  ON_AFTER_DELETE_PROJECT_REGISTRY = "nul:on_after_delete_project_registry",

  ----------------------------------------------------------------------
  -- UCM.nvim (コードジェネレーターなど) が発行する可能性のあるイベント
  ----------------------------------------------------------------------
  ON_SOURCE_FILE_CREATED = "unl:on_source_file_created",
  ON_SOURCE_FILE_DELETED = "unl:on_source_file_deleted",
  ON_PLUGIN_AFTER_SETUP = "unl:on_plugin_after_setup",

  ON_BEFORE_PROGRESS_WRITE = "unl:on_before_progress_write",
  ON_AFTER_PROGRESS_WRITE = "unl:on_after_progress_write",
  
  ON_BEFORE_BUILD = "unl:on_before_build",
  ON_AFTER_BUILD = "unl:on_after_build",

  ON_AFTER_GENERATE_COMPILE_DATABASE = "unl:on_generate_compile_database",
  ON_AFTER_GENERATE_HEADER = "unl:on_generate_header",
  ON_AFTER_GENERATE_PROEJCT = "unl:on_after_generate_proejct",
  ON_AFTER_LINT = "unl:on_after_lint",

  ON_AFTER_DELETE_CLASS_FILE = "unl:on_after_delete_class_file",
  ON_AFTER_NEW_CLASS_FILE = "unl:on_after_new_class_file",
  ON_AFTER_MOVE_CLASS_FILE = "unl:on_after_move_class_file",
  ON_AFTER_RENAME_CLASS_FILE = "unl:on_after_rename_class_file",

  ON_REQUEST_UPROJECT_TREE_VIEW = "unl:on_request_uproject_tree_view",

  ON_AFTER_LOG_VIEWER_START = "unl:on_after_log_viewer_start",
  ON_AFTER_LOG_VIEWER_STOP = "unl:on_after_log_viewer_stop",

  ON_REQUEST_TRACE_CALLEES_VIEW = "unl:on_request_trace_callees_view",
  
  -- ★追加: ディレクトリ変更イベント
  ON_AFTER_MODIFY_DIRECTORY = "unl:on_after_modify_directory",

  ON_AFTER_NEW_PROJECT = "unl:on_after_new_project",
}

return M
