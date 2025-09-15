-- lua/UNL/command/builder.lua (完全版)

local M = {}

function M.create(spec)
  -- 0. 依存関係とロガーの準備
  local function get_logger()
    return require("UNL.logging").get(spec.plugin_name or "UNL")
  end

  if spec.dependencies then
    for _, dep in ipairs(spec.dependencies) do
      local ok, result = pcall(dep.check)
      if not (ok and result) then
        local error_msg = ("%s requires '%s'. %s"):format(spec.cmd_name, dep.name, dep.msg or "")
        get_logger().error(error_msg)
        return
      end
    end
  end

  -- 1. ロードガード (二重読み込み防止)
  local guard_key = "loaded_cmd_" .. spec.cmd_name:lower()
  if vim.g[guard_key] then return end
  vim.g[guard_key] = true

  -- 2. コマンドハンドラ本体
  local function command_handler(args)
    -- args = { fargs, bang, ... }

    -- 2a. サブコマンドが指定されていない場合は使い方を表示
    if not args.fargs or #args.fargs == 0 then
      get_logger().warn("Usage: :" .. spec.cmd_name .. " <subcommand> ...")
      return
    end

    -- 2b. bang(!)の有無を確定させる
    local has_bang = (args.bang == "!") or (args.fargs[1] and args.fargs[1]:match("!$") ~= nil)

    -- 2c. サブコマンド名を安全に抽出する
    local sub_name_raw = args.fargs[1]
    local sub_name = sub_name_raw:gsub("!$", "")

    -- 2d. サブコマンド定義を検索
    local command_def = spec.subcommands[sub_name:lower()]
    if not command_def then
      get_logger().error("Unknown subcommand: " .. sub_name_raw)
      return
    end
    
    -- 2e. コマンド定義がbang(!)をサポートしているかチェック
    if has_bang and not command_def.bang then
      get_logger().error(("Subcommand '%s' does not support bang (!)."):format(sub_name))
      return
    end

    -- 2f. ハンドラに渡す `opts` テーブルを準備
    local opts = { has_bang = has_bang }
    -- ▼▼▼ ここからが修正箇所です ▼▼▼

    -- 2g. ユーザー引数を「位置引数」と「フラグ引数」に分類する
    local positional_args = {}
    local flag_args = {}
    for i = 2, #args.fargs do
      local arg = args.fargs[i]
      if arg:sub(1, 2) == "--" then
        table.insert(flag_args, arg)
      else
        table.insert(positional_args, arg)
      end
    end

    -- 2h. 定義に基づいて引数をパース
    if command_def.args then
      local positional_idx = 1
      for _, arg_def in ipairs(command_def.args) do
        -- specの引数名が "_flag" で終わる場合、フラグとして扱う
        if arg_def.name:match("_flag$") then
          local found = false
          -- 登録されているフラグ引数の中から一致するものを探す
          for _, flag in ipairs(flag_args) do
            -- ここでは単純な完全一致のみを考慮する
            -- 例: --all-deps, --no-deps
            opts[arg_def.name] = flag
            found = true
            break -- 複数フラグがあっても最初のものを使う
          end
          if not found and arg_def.required then
            get_logger().error("Missing required flag for argument: '%s'", arg_def.name)
            return
          end
        else
          -- フラグでなければ、位置引数として扱う
          local value = positional_args[positional_idx]
          if value == nil then
            if arg_def.required then
              get_logger().error("Missing required argument: '%s'. Usage: %s", arg_def.name, command_def.desc or "")
              return
            end
            if type(arg_def.default) == "function" then
              value = arg_def.default()
            else
              value = arg_def.default
            end
          end
          opts[arg_def.name] = value
          positional_idx = positional_idx + 1
        end
      end
    end

    -- ▲▲▲ ここまでが修正箇所です ▲▲▲
    
    -- 2i. 最終的なハンドラを実行
    command_def.handler(opts)
  end

  -- 3. 補完ハンドラ
  local function complete_handler(arglead, cmdline, cursorpos)
    local parts = vim.split(cmdline, " ", true)
    -- サブコマンド名を補完
    if #parts <= 2 then
      return vim.tbl_filter(function(name)
        return vim.startswith(name, arglead)
      end, vim.tbl_keys(spec.subcommands))
    end
    -- (将来的に各サブコマンドの引数補完もここに追加可能)
    return {}
  end

  -- 4. コマンド登録
  vim.api.nvim_create_user_command(spec.cmd_name, command_handler, {
    nargs = "*",
    bang = true,
    desc = spec.desc or (spec.cmd_name .. " commands"),
    complete = complete_handler,
  })

  get_logger().info("%s commands registered.", spec.cmd_name)
end

return M
