-- lua/UNL/parser/query.lua
local M = {}

M.cpp_structure = [[
  ;; ---------------------------------------------------------
  ;; クラス/構造体/列挙型
  ;; ---------------------------------------------------------
  (class_specifier name: (_) @class_name) @class_def
  (struct_specifier name: (_) @struct_name) @struct_def
  (enum_specifier name: (_) @enum_name) @enum_def
  
  ;; UE固有の宣言
  (unreal_class_declaration name: (_) @class_name) @uclass_def
  (unreal_struct_declaration name: (_) @struct_name) @ustruct_def
  (unreal_enum_declaration name: (_) @enum_name) @uenum_def
  
  ;; DECLARE_CLASS マクロ
  (unreal_declare_class_macro) @declare_class_macro

  ;; ---------------------------------------------------------
  ;; 関数定義 (Header / Source)
  ;; ---------------------------------------------------------
  ;; 通常の関数宣言 (戻り値あり/なし/ポインタ/参照)
  (function_definition
    declarator: [
      (function_declarator declarator: (_) @func_name)
      (pointer_declarator (function_declarator declarator: (_) @func_name))
      (reference_declarator (function_declarator declarator: (_) @func_name))
      (field_identifier) @func_name
      (identifier) @func_name
      ;; スコープ付き定義 (MyClass::Method)
      (function_declarator (qualified_identifier scope: (_) @impl_class name: (_) @func_name))
      (pointer_declarator (function_declarator (qualified_identifier scope: (_) @impl_class name: (_) @func_name)))
      (reference_declarator (function_declarator (qualified_identifier scope: (_) @impl_class name: (_) @func_name)))
    ]
  ) @method_def

  ;; 宣言のみ (Header内のプロトタイプなど)
  (field_declaration
    declarator: [
      (function_declarator declarator: (_) @func_name)
      (pointer_declarator (function_declarator declarator: (_) @func_name))
      (reference_declarator (function_declarator declarator: (_) @func_name))
    ]
  ) @method_decl

  ;; 関数宣言 (トップレベルや特殊なケース)
  (declaration
    (function_declarator
      declarator: (_) @func_name
    )
  ) @method_decl

  ;; ---------------------------------------------------------
  ;; メンバ変数 / フィールド
  ;; ---------------------------------------------------------
  (field_declaration
    declarator: [
      (field_identifier) @field_name
      (pointer_declarator declarator: (_) @field_name)
      (pointer_declarator (_) @field_name)
      (array_declarator declarator: (_) @field_name)
      (array_declarator (_) @field_name)
      (reference_declarator (_) @field_name)
    ]
  ) @field_decl

  ;; ---------------------------------------------------------
  ;; その他
  ;; ---------------------------------------------------------
  (access_specifier) @access_label
]]

return M
