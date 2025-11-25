-- lua/UNL/parser/query.lua
local M = {}

M.cpp_structure = [[
  ;; クラス/構造体/列挙型の定義
  (class_specifier name: (_) @class_name) @class_def
  (struct_specifier name: (_) @struct_name) @struct_def
  (enum_specifier name: (_) @enum_name) @enum_def
  
  ;; UE固有のマクロ定義
  (unreal_class_declaration name: (_) @class_name) @uclass_def
  (unreal_struct_declaration name: (_) @struct_name) @ustruct_def
  (unreal_enum_declaration name: (_) @enum_name) @uenum_def
  
  ;; DECLARE_CLASS マクロ (UObject等)
  (unreal_declare_class_macro) @declare_class_macro

  ;; メンバ関数の宣言 (ヘッダー内)
  ;; virtual や type の取得は Lua 側で行うため、ここでは構造のマッチングのみ行う
  (field_declaration
    declarator: [
      (function_declarator declarator: (_) @func_name parameters: (_) @params)
      (pointer_declarator (function_declarator declarator: (_) @func_name parameters: (_) @params))
      (reference_declarator (function_declarator declarator: (_) @func_name parameters: (_) @params))
    ]
  ) @method_decl

  ;; アクセス指定子
  (access_specifier) @access_label
]]

return M
